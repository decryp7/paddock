//! Debrief 1.2 regression tests, store side (docs/plans/debrief-1-2.md, wave 0 package 0.3; ADR 0018 and 0051).
//!
//! Each test is written to fail today and is `#[ignore]`d with the failure it shows, until the work package that fixes
//! it removes the marker (3.5 for the `.gz`, 4.2 for the exception groups, P3 for the 1.6 M-row catalogue). The inputs are generated at test time into
//! a temp folder (invented rows, never committed, never real data) and the big one is slow, so the tests only run when
//! asked for: `cargo test --release -p debrief-store-duck --test regress_1_2 -- --ignored` (nightly/bench workflow).

mod common;

use common::rss::Sampler;
use common::TempDir;
use debrief_core::api::QueryRequest;
use debrief_core::catalogue::CatalogueRequest;
use debrief_core::ports::{Analysis, Query};
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
    assert_eq!(
        json["truncated"]["limit"].is_null(),
        false,
        "the answer says it was truncated: {:?}",
        answer.coverage
    );
}

/// Rows of the permanent catalogue of a store that imported about 7.6 GB (docs/research/2026-10-28-cpu-memory.md, H3).
const CATALOGUE_ROWS: u64 = 1_600_000;

#[test]
#[ignore = "regress-1-2: FAILING TODAY (fixed by plan package P3, bounded catalogue read): catalogue(all) over 1600000 rows: Out of Memory Error: could not allocate block of size 256.0 KiB (183.0 MiB/183.1 MiB used)"]
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
    let reply = store.catalogue(&CatalogueRequest {
        all: true,
        ..CatalogueRequest::default()
    });
    let reply = reply.unwrap_or_else(|e| panic!("catalogue(all) over {CATALOGUE_ROWS} rows failed within the 192 MB query budget: {e}"));
    assert!(reply.built, "the catalogue is built");
    assert!(
        reply.candidates.len() <= debrief_core::catalogue::MAX_NAMES,
        "{} candidates; a read returns at most {}",
        reply.candidates.len(),
        debrief_core::catalogue::MAX_NAMES
    );
}
