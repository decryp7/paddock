//! The persistent catalogue: names found only in event text are resolved, kept up to date by imports, removals and
//! clears, and looked up with typos.

use crate::common;

use common::TempDir;
use debrief_core::catalogue::{CatalogueRequest, Scope};
use debrief_core::entities::EntityKind;
use debrief_core::find::{find, resolve_terms, Via, ENTITY_FIELD};
use debrief_core::ports::{Ingest, Query, Tail};
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::path::Path;
use std::time::Duration;

fn lines(path: &Path, rows: &[&str]) {
    let mut text = String::new();
    for (i, r) in rows.iter().enumerate() {
        text.push_str(&format!("2026-10-14 10:00:{:02} INFO {r}\n", i % 60));
    }
    std::fs::write(path, text).unwrap();
}

fn built(store: &DuckStore) {
    assert!(store.wait_for_catalogue(Duration::from_secs(60)).unwrap(), "catalogue built");
}

fn lookup(store: &DuckStore, text: &str) -> Vec<(Scope, String, u64)> {
    store
        .catalogue(&CatalogueRequest {
            text: text.into(),
            ..Default::default()
        })
        .unwrap()
        .candidates
        .into_iter()
        .map(|c| (c.scope, c.name, c.count))
        .collect()
}

fn setup(name: &str) -> (TempDir, TempDir, DuckStore) {
    let logs = TempDir::new(&format!("cat-logs-{name}"));
    let dir = TempDir::new(&format!("cat-store-{name}"));
    let store = DuckStore::create(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    (logs, dir, store)
}

#[test]
fn a_name_only_in_message_text_is_resolved_with_typos_and_kind() {
    let (logs, _dir, store) = setup("text");
    lines(
        &logs.0.join("a.log"),
        &[
            "Started CompanyA.Graphic.Builder.exe from C:\\Tools\\Acme\\Graphic.Builder.exe",
            "CompanyA.Graphic.Builder.exe crashed with System.TimeoutException, HRESULT 0x80070005",
            "connect to api.example.com failed for 10.1.2.3",
            "heartbeat",
        ],
    );
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);

    for text in ["graphic builder", "grafic bilder", "GraphicBuilder"] {
        let v = resolve_terms(&store, text, 5).unwrap();
        let top = v
            .iter()
            .find(|m| m.value == "CompanyA.Graphic.Builder.exe")
            .unwrap_or_else(|| panic!("{text}: {v:#?}"));
        assert_eq!(
            (top.field.as_str(), top.kind, top.count),
            (ENTITY_FIELD, Some(EntityKind::Executable), 2),
            "{text}"
        );
        assert_eq!(top.seen_in, ["a.log"]);
        assert!(top.first_seen.is_some() && top.last_seen >= top.first_seen);
    }
    let typo = resolve_terms(&store, "grafic bilder", 5).unwrap();
    assert_eq!(typo[0].via, Via::Fuzzy);
    let r = find(&store, "timeoutexception", 0, 5).unwrap();
    assert!(r.total >= 1, "{r:#?}");
    let hosts = lookup(&store, "api example");
    assert!(
        hosts
            .iter()
            .any(|(s, n, c)| *s == Scope::Entity(EntityKind::Host) && n == "api.example.com" && *c == 1),
        "{hosts:?}"
    );
    assert!(lookup(&store, "0x80070005")
        .iter()
        .any(|(s, ..)| *s == Scope::Entity(EntityKind::ErrorCode)));
    assert!(lookup(&store, "10.1.2.3").iter().any(|(s, ..)| *s == Scope::Entity(EntityKind::Ip)));
    assert!(lookup(&store, "zz").is_empty() && lookup(&store, "  ").is_empty());
    assert!(!lookup(&store, "Co").is_empty(), "a short text matches name starts");
}

#[test]
fn the_catalogue_follows_imports_removals_and_clear() {
    let (logs, _dir, store) = setup("life");
    lines(&logs.0.join("a.log"), &["ran alpha.exe", "ran alpha.exe"]);
    let first = store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    assert_eq!(lookup(&store, "alpha")[0].2, 2);

    // A second file adds to the counts (incremental) and a new name appears.
    lines(&logs.0.join("b.log"), &["ran alpha.exe", "ran beta.dll"]);
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    assert_eq!(lookup(&store, "alpha")[0].2, 3);
    assert_eq!(lookup(&store, "beta")[0].1, "beta.dll");

    // Removing a source takes its names with it, at once.
    let id = store.summary().unwrap().sources.iter().find(|s| s.name == "b.log").unwrap().id;
    store.remove_source(debrief_core::batch::SourceId(id)).unwrap();
    assert_eq!(lookup(&store, "alpha")[0].2, 2);
    assert!(lookup(&store, "beta").is_empty());

    store.clear().unwrap();
    assert!(lookup(&store, "alpha").is_empty());
    let reply = store
        .catalogue(&CatalogueRequest {
            text: "alpha".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(reply.built, "nothing to catalogue is built");

    // Reload (a re-import) keeps the names consistent: no doubled counts.
    lines(&logs.0.join("a.log"), &["ran alpha.exe", "ran alpha.exe"]);
    std::fs::remove_file(logs.0.join("b.log")).unwrap();
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    store.reload().unwrap().wait().unwrap();
    built(&store);
    assert_eq!(lookup(&store, "alpha")[0].2, 2);
    drop(first);
}

/// The catalogue's rows are checkpointed once it is built, like every other change to the store: with the store's 1 GB
/// `checkpoint_threshold` they otherwise stayed in DuckDB's memory and the write-ahead log, and a worker that had
/// catalogued 20 copies of the samples idled at about 440 MB against a 300 MB budget (docs/performance.md).
#[test]
fn a_built_catalogue_is_saved_into_the_database_file() {
    let (logs, dir, store) = setup("saved");
    let rows: Vec<String> = (0..200).map(|i| format!("ran tool{i}.exe on host{i}.example.com")).collect();
    lines(&logs.0.join("a.log"), &rows.iter().map(String::as_str).collect::<Vec<_>>());
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    assert!(!lookup(&store, "tool7").is_empty(), "the names are catalogued");
    let wal = dir.0.join(format!("{}.wal", debrief_store_duck::DB_FILE));
    let left = std::fs::metadata(&wal).map_or(0, |m| m.len());
    assert_eq!(
        left, 0,
        "the built catalogue left {left} bytes in the write-ahead log (not checkpointed)"
    );
}

#[test]
fn field_values_are_catalogued_and_numbers_and_ids_are_not() {
    let (logs, _dir, store) = setup("fields");
    let mut text = String::new();
    for i in 0..30 {
        text.push_str(&format!(
            "{{\"@t\":\"2026-10-14T10:00:{i:02}Z\",\"@mt\":\"event {i}\",\"Service\":\"billing-api\",\"Count\":\"{i}\"}}\n"
        ));
    }
    std::fs::write(logs.0.join("a.clef"), text).unwrap();
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    let v = lookup(&store, "billing api");
    assert!(
        v.iter()
            .any(|(s, n, c)| *s == Scope::Field("Service".into()) && n == "billing-api" && *c == 30),
        "{v:?}"
    );
    let r = resolve_terms(&store, "billing api", 3).unwrap();
    assert_eq!((r[0].field.as_str(), r[0].kind), ("Service", None));
    assert!(lookup(&store, "event")
        .iter()
        .all(|(s, ..)| !matches!(s, Scope::Field(f) if f == "Count")));
}

#[test]
fn an_unbuilt_catalogue_falls_back_to_facets_and_reports_pending() {
    let (_logs, _dir, store) = setup("empty");
    let reply = store
        .catalogue(&CatalogueRequest {
            text: "x".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(reply.built && reply.candidates.is_empty() && reply.pending == 0);
}

/// A source whose build failed is given up on, not retried until the store is reopened: it counts as done (the catalogue is
/// built without it, and a wait for the catalogue ends) and is reported, where a source that is only not yet catalogued is
/// pending.
#[test]
fn a_source_whose_build_failed_counts_as_done_and_is_reported() {
    let (logs, _dir, store) = setup("failed");
    lines(&logs.0.join("a.log"), &["Started CompanyA.Graphic.Builder.exe"]);
    lines(&logs.0.join("b.log"), &["Started CompanyA.Tray.Agent.exe"]);
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    let status = || store.catalogue_status(&Default::default()).unwrap();
    let sources: Vec<u32> = store.summary().unwrap().sources.iter().map(|s| s.id).collect();
    assert_eq!(sources.len(), 2);
    assert_eq!((status().built, status().pending, status().failed), (true, 0, 0));
    // One source failed: done and reported.
    store.catalogue_give_up_on(sources[0]).unwrap();
    let s = status();
    assert_eq!(
        (s.built, s.pending, s.failed),
        (true, 0, 1),
        "a failed source does not keep the catalogue from being built"
    );
    let reply = store.catalogue(&CatalogueRequest::default()).unwrap();
    assert_eq!((reply.built, reply.pending), (true, 0));
    // Another one that is only missing from the catalogue (not given up on) is pending, and the catalogue is not built.
    store
        .bench_sql(&format!("DELETE FROM catalogue_sources WHERE source = {}", sources[1]))
        .unwrap();
    let s = status();
    assert_eq!((s.built, s.pending, s.failed), (false, 1, 1));
    let reply = store.catalogue(&CatalogueRequest::default()).unwrap();
    assert_eq!((reply.built, reply.pending), (false, 1));
}

#[test]
fn every_name_is_listed_for_the_name_book_and_graphic_builder_groups_its_spellings() {
    use debrief_core::names::Names;
    let (logs, _dir, store) = setup("all");
    lines(
        &logs.0.join("a.log"),
        &[
            "Started CompanyA.Graphic.Builder.exe",
            "graphicbuilder.exe rendered a frame",
            "graphicbuilder.exe rendered a frame",
            "Started CompanyA.Tray.Agent.exe",
            "heartbeat",
        ],
    );
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    let all = store
        .catalogue(&CatalogueRequest {
            all: true,
            ..Default::default()
        })
        .unwrap();
    assert!(all.built && all.candidates.len() >= 3, "{:?}", all.candidates);
    assert!(all.candidates.windows(2).all(|w| w[0].count >= w[1].count), "most events first");

    let names = Names::default();
    assert!(names.refresh(&store).unwrap() >= 2);
    let layer_find = |text: &str| {
        let ctx = names.context().clone();
        names.read(|book, vocab| {
            debrief_core::find::resolve_names(
                &store,
                text,
                5,
                Some(&debrief_core::find::NameLayer {
                    book,
                    vocab,
                    ctx: &ctx,
                    settings: names.settings(),
                }),
            )
            .unwrap()
        })
    };
    let r = layer_find("graphic builder");
    assert_eq!(r.values[0].group.as_deref(), Some("executable|graphic builder"), "{r:#?}");
    assert_eq!(r.values[0].spellings.len(), 2);
    assert!(!r.values[0].evidence.is_empty());
}

/// A live-follow step (`append_tail`) bumps the store's generation; the idle catalogue loop runs no statement without
/// news (turns::Gate), so the step must reach it as news: a file that appears under the opened folder is catalogued
/// without anything waking the thread by hand (`wait_for_catalogue` would). Lines appended to a file the catalogue already
/// finished are not read again: the catalogue is built per source, as before this change.
#[test]
fn a_file_that_live_follow_adds_is_catalogued_without_a_manual_wake() {
    let (logs, _dir, store) = setup("tail");
    let first = logs.0.join("a.log");
    lines(&first, &["Started Alpha.Service.exe", "heartbeat"]);
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    built(&store);
    assert!(lookup(&store, "Quokkasoft").is_empty(), "the fixture holds no such name yet");
    // The catalogue is built and its loop idle: now a file appears and the followed one grows.
    let added = logs.0.join("b.log");
    lines(&added, &["Started Quokkasoft.Agent.exe"]);
    let mut text = std::fs::read_to_string(&first).unwrap();
    text.push_str("2026-10-14 10:01:00 INFO heartbeat again\n");
    std::fs::write(&first, text).unwrap();
    let appended = store.append_tail(&[first, added]).unwrap();
    assert!(appended.appended > 0, "the step read the new lines: {appended:?}");
    let catalogued = paddock_testkit::poll_until_within(Duration::from_secs(30), || !lookup(&store, "Quokkasoft").is_empty());
    assert!(
        catalogued,
        "the name in the new file was not catalogued: {:?}",
        lookup(&store, "Quokkasoft")
    );
}
