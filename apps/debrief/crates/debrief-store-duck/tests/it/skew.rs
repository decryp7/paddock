//! Clock-skew correction on the store: an offset moves a file's events (ids stay), survives a reopen, applies to
//! files read later and to appended lines, refuses bad requests with what to fix, and the store suggests and
//! applies the same offsets as the in-memory reference.

use crate::common;

use common::{assert_blocks_consistent, assert_blocks_consistent_with, pair_of, store_with, TempDir};
use debrief_core::ports::{Filter, Ingest, Maintain, MaintenanceAction, PageQuery, Positions, Query, Search, SearchRequest, Skew};
use debrief_core::skew::{OffsetSpec, SetOffsetRequest, SuggestRequest};
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::io::Write;

fn manual(source: u32, offset_ms: i64) -> SetOffsetRequest {
    SetOffsetRequest {
        spec: OffsetSpec::Manual { source, offset_ms },
    }
}

fn times(store: &DuckStore) -> Vec<(String, Option<i64>)> {
    store
        .page(&PageQuery {
            limit: 100,
            ..PageQuery::default()
        })
        .unwrap()
        .rows
        .iter()
        .map(|r| (r.message.clone(), r.ts))
        .collect()
}

fn source_of(store: &DuckStore, file: &str) -> u32 {
    store.summary().unwrap().sources.iter().find(|s| s.path.ends_with(file)).unwrap().id
}

/// The rewritten segments keep their id blocks (ADR 0062 section 1): the blocks move to the new files in the same
/// catalog transaction, so every row still lies in a block of its own file and no two blocks overlap.
#[test]
fn an_offset_moves_the_id_blocks_with_the_rows() {
    let logs = TempDir::new("skew-blocks-logs");
    std::fs::write(logs.0.join("a.log"), "2026-10-14 10:00:00 INFO a1\n2026-10-14 10:00:10 INFO a2\n").unwrap();
    std::fs::write(logs.0.join("b.log"), "2026-10-14 10:00:05 INFO b1\n2026-10-14 10:00:15 INFO b2\n").unwrap();
    let dir = TempDir::new("skew-blocks-store");
    let (store, _) = store_with(&dir, Profile::default_profile(), std::slice::from_ref(&logs.0)).unwrap();
    assert_blocks_consistent(&store, &dir.0);
    let blocks = store.bench_sql("SELECT 1 FROM segment_blocks").unwrap();
    let files = store.bench_sql("SELECT file FROM segments").unwrap();
    let b = source_of(&store, "b.log");
    store.set_source_offset(&manual(b, 20_000)).unwrap();
    assert_blocks_consistent(&store, &dir.0);
    assert_eq!(
        store.bench_sql("SELECT 1 FROM segment_blocks").unwrap(),
        blocks,
        "every block is kept"
    );
    assert_eq!(store.bench_sql("SELECT file FROM segments").unwrap(), files);
}

/// The full-text index holds each event's time and orders hits by it (ADR 0062, defect 3): after an offset the moved
/// events are indexed again with their new times, so hits come back once each, in the new time order.
#[test]
fn search_hits_follow_an_offset_in_time_order() {
    let logs = TempDir::new("skew-search-logs");
    std::fs::write(
        logs.0.join("a.log"),
        "2026-10-14 10:00:00 INFO alpha a1\n2026-10-14 10:00:10 INFO alpha a2\n",
    )
    .unwrap();
    std::fs::write(
        logs.0.join("b.log"),
        "2026-10-14 10:00:05 INFO alpha b1\n2026-10-14 10:00:15 INFO alpha b2\n",
    )
    .unwrap();
    let dir = TempDir::new("skew-search-store");
    let (store, _) = store_with(&dir, Profile::default_profile(), std::slice::from_ref(&logs.0)).unwrap();
    let wait = |store: &DuckStore| assert!(store.wait_for_index(std::time::Duration::from_secs(60)).unwrap().ready);
    let hits = |store: &DuckStore| {
        store
            .search(&SearchRequest {
                text: "alpha".into(),
                limit: 0,
            })
            .unwrap()
    };
    let in_time_order = |store: &DuckStore| {
        store
            .page(&PageQuery {
                limit: 10,
                ..PageQuery::default()
            })
            .unwrap()
            .rows
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    wait(&store);
    let before = hits(&store);
    assert_eq!((before.total, &before.ids), (4, &in_time_order(&store)));
    store.set_source_offset(&manual(source_of(&store, "b.log"), 20_000)).unwrap();
    let order: Vec<String> = times(&store).into_iter().map(|(m, _)| m).collect();
    assert_eq!(
        order,
        ["alpha a1", "alpha a2", "alpha b1", "alpha b2"],
        "b's events moved after a's"
    );
    wait(&store);
    let after = hits(&store);
    assert_eq!(after.total, 4, "each event once");
    assert_eq!(after.ids, in_time_order(&store), "hits in the new time order");
}

/// Segments from before `segment_blocks` whose ranges overlap get no blocks when the store opens (left for ADR 0062's
/// WP-4 scan), and a block of a file no longer listed is dropped. Compaction leaves blockless segments alone, so no
/// merged file holds only part of its blocks, and an offset afterwards neither loses nor doubles a hit.
#[test]
fn overlapping_legacy_segments_are_not_compacted_and_an_offset_keeps_one_hit_each() {
    let logs = TempDir::new("legacy-blocks-logs");
    for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
        std::fs::write(
            logs.0.join(format!("{name}.log")),
            format!("2026-10-14 10:00:0{i} INFO alpha {name}1\n2026-10-14 10:01:0{i} INFO alpha {name}2\n"),
        )
        .unwrap();
    }
    let dir = TempDir::new("legacy-blocks-store");
    let store = DuckStore::create_with(
        &dir.0,
        Profile::default_profile(),
        PluginRegistry::new(),
        debrief_store_duck::StoreOptions {
            parse_threads: 1,
            ..debrief_store_duck::StoreOptions::default()
        },
    )
    .unwrap();
    let wait = |store: &DuckStore| assert!(store.wait_for_index(std::time::Duration::from_secs(60)).unwrap().ready);
    for name in ["a", "b", "c", "d"] {
        store.open(&[logs.0.join(format!("{name}.log"))]).unwrap().wait().unwrap();
        wait(&store);
    }
    assert_eq!(store.bench_sql("SELECT 1 FROM segments").unwrap(), 4, "one segment per import");
    drop(store);
    // As an older store: the first two segments' ranges overlap (a compacted hull can), they have no blocks yet, and a
    // block of a file that is gone is left over.
    {
        let conn = duckdb::Connection::open(dir.0.join(debrief_store_duck::DB_FILE)).unwrap();
        let mut stmt = conn.prepare("SELECT file, end_id FROM segments ORDER BY first_id LIMIT 2").unwrap();
        let first: Vec<(String, u64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        conn.execute(
            "UPDATE segments SET end_id = ? WHERE file = ?",
            duckdb::params![first[1].1, first[0].0],
        )
        .unwrap();
        for (file, _) in &first {
            conn.execute("DELETE FROM segment_blocks WHERE file = ?", duckdb::params![file])
                .unwrap();
        }
        conn.execute_batch("INSERT INTO segment_blocks VALUES ('seg-gone.parquet', 1, 2)")
            .unwrap();
    }
    let store = DuckStore::open(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    assert_eq!(
        store
            .bench_sql("SELECT 1 FROM segment_blocks WHERE file = 'seg-gone.parquet'")
            .unwrap(),
        0,
        "the block of a file that is gone is dropped"
    );
    assert_eq!(
        store
            .bench_sql("SELECT 1 FROM segments s WHERE NOT EXISTS (SELECT 1 FROM segment_blocks b WHERE b.file = s.file)")
            .unwrap(),
        2,
        "overlapping ranges wait for the scan"
    );
    assert_blocks_consistent_with(&store, &dir.0, true);
    store.maintain(MaintenanceAction::Compact).unwrap();
    assert_blocks_consistent_with(&store, &dir.0, true);
    assert_eq!(
        store
            .bench_sql("SELECT 1 FROM segments s WHERE NOT EXISTS (SELECT 1 FROM segment_blocks b WHERE b.file = s.file)")
            .unwrap(),
        2,
        "blockless segments are not compacted"
    );
    assert_eq!(
        store.bench_sql("SELECT 1 FROM segments").unwrap(),
        3,
        "the two with blocks were merged"
    );
    for name in ["a", "c"] {
        store
            .set_source_offset(&manual(source_of(&store, &format!("{name}.log")), 300_000))
            .unwrap();
    }
    wait(&store);
    assert_blocks_consistent_with(&store, &dir.0, true);
    let hits = store
        .search(&SearchRequest {
            text: "alpha".into(),
            limit: 0,
        })
        .unwrap();
    assert_eq!(hits.total, 8, "every event once");
    let mut ids = hits.ids.clone();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 8);
}

#[test]
fn an_offset_moves_a_files_events_and_is_kept_with_the_store() {
    let logs = TempDir::new("skew-logs");
    std::fs::write(logs.0.join("a.log"), "2026-10-14 10:00:00 INFO a1\n2026-10-14 10:00:10 INFO a2\n").unwrap();
    std::fs::write(logs.0.join("b.log"), "2026-10-14 10:00:05 INFO b1\n2026-10-14 10:00:15 INFO b2\n").unwrap();
    let dir = TempDir::new("skew-store");
    let (store, _) = store_with(&dir, Profile::default_profile(), std::slice::from_ref(&logs.0)).unwrap();
    let before = times(&store);
    let order: Vec<&str> = before.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(order, ["a1", "b1", "a2", "b2"]);
    let ids_before: Vec<_> = store
        .page(&PageQuery {
            limit: 10,
            ..PageQuery::default()
        })
        .unwrap()
        .rows
        .iter()
        .map(|r| (r.message.clone(), r.id))
        .collect();
    let b = source_of(&store, "b.log");

    let change = store.set_source_offset(&manual(b, 20_000)).unwrap();
    assert_eq!(
        (change.source, change.offset_ms, change.previous_ms, change.ids_changed_from),
        (b, 20_000, 0, None)
    );
    // Events keep their ids when they move in time.
    let ids_after: std::collections::HashMap<_, _> = store
        .page(&PageQuery {
            limit: 10,
            ..PageQuery::default()
        })
        .unwrap()
        .rows
        .iter()
        .map(|r| (r.message.clone(), r.id))
        .collect();
    for (m, id) in &ids_before {
        assert_eq!(ids_after[m], *id, "{m} kept its id");
    }
    let after = times(&store);
    let order: Vec<&str> = after.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(order, ["a1", "a2", "b1", "b2"], "b's events moved after a's");
    let shift = |m: &str| after.iter().find(|r| r.0 == m).unwrap().1.unwrap() - before.iter().find(|r| r.0 == m).unwrap().1.unwrap();
    assert_eq!((shift("a1"), shift("b1"), shift("b2")), (0, 20_000, 20_000));
    let ids_after: Vec<_> = store
        .page(&PageQuery {
            limit: 10,
            ..PageQuery::default()
        })
        .unwrap()
        .rows
        .iter()
        .map(|r| (r.message.clone(), r.id))
        .collect();
    for (m, id) in &ids_before {
        assert_eq!(ids_after.iter().find(|x| &x.0 == m).unwrap().1, *id, "{m} keeps its id");
    }
    let summary = store.summary().unwrap();
    let info = summary.sources.iter().find(|s| s.id == b).unwrap();
    assert_eq!(info.offset_ms, 20_000);
    assert_eq!(
        (info.from, info.to),
        (Some(before[1].1.unwrap() + 20_000), Some(before[3].1.unwrap() + 20_000))
    );
    assert_eq!(summary.to, Some(before[3].1.unwrap() + 20_000));
    assert_eq!(store.positioned().unwrap(), 4);
    assert_eq!(store.clock_offsets().unwrap().get(&info.path), 20_000);

    // Another change moves by the difference only.
    let c = store.set_source_offset(&manual(b, 5_000)).unwrap();
    assert_eq!((c.previous_ms, c.offset_ms), (20_000, 5_000));
    assert_eq!(shift_of(&store, &before, "b1"), 5_000);

    // Refused requests change nothing and say what to fix.
    for (req, wants) in [
        (manual(b, 8 * 24 * 3_600_000), "too large"),
        (manual(9_999, 1), "not open any more"),
    ] {
        let e = store.set_source_offset(&req).unwrap_err().to_string();
        assert!(e.contains(wants), "{e}");
    }
    assert_eq!(store.clock_offsets().unwrap().get(&info.path), 5_000);

    // Lines appended later take the offset.
    let mut f = std::fs::OpenOptions::new().append(true).open(logs.0.join("b.log")).unwrap();
    f.write_all(b"2026-10-14 10:00:30 INFO b3\n2026-10-14 10:00:40 INFO b4\n").unwrap();
    drop(f);
    store.append_tail(&[logs.0.join("b.log")]).unwrap();
    store.append_tail(&[]).unwrap();
    let now = times(&store);
    let b3 = now.iter().find(|r| r.0 == "b3").unwrap().1.unwrap();
    assert_eq!(b3, utc("2026-10-14 10:00:30") + 5_000);
    assert_eq!(store.positioned().unwrap(), 6);

    // Kept with the store: a reopened store still holds the corrected times and the offset.
    drop(store);
    let reopened = DuckStore::open(&dir.0, Profile::default_profile(), PluginRegistry::new()).unwrap();
    assert_eq!(reopened.clock_offsets().unwrap().get(&info.path), 5_000);
    assert_eq!(times(&reopened).iter().find(|r| r.0 == "b3").unwrap().1, Some(b3));

    // A saved project repeats the offsets.
    let saved = debrief_core::ports::Projects::save_project(&reopened, &debrief_core::package::ProjectManifest::default());
    if let Ok(m) = saved {
        assert_eq!(
            m.clock_offsets.iter().map(|o| (o.path.clone(), o.offset_ms)).collect::<Vec<_>>(),
            [(info.path.clone(), 5_000)]
        );
    }

    // Back to none: the original times.
    let c = reopened.set_source_offset(&manual(b, 0)).unwrap();
    assert_eq!(c.previous_ms, 5_000);
    assert!(reopened.clock_offsets().unwrap().is_empty());
    assert_eq!(times(&reopened).iter().find(|r| r.0 == "b1").unwrap().1, before[1].1);
}

fn shift_of(store: &DuckStore, before: &[(String, Option<i64>)], m: &str) -> i64 {
    times(store).iter().find(|r| r.0 == m).unwrap().1.unwrap() - before.iter().find(|r| r.0 == m).unwrap().1.unwrap()
}

fn utc(text: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
        .unwrap()
        .and_utc()
        .timestamp_millis()
}

#[test]
fn files_read_later_and_restored_states_take_the_offsets() {
    let logs = TempDir::new("skew-later-logs");
    std::fs::write(logs.0.join("a.log"), "2026-10-14 10:00:00 INFO a1\n").unwrap();
    let dir = TempDir::new("skew-later-store");
    let (store, _) = store_with(&dir, Profile::default_profile(), &[]).unwrap();
    let path = std::fs::canonicalize(logs.0.join("a.log")).unwrap().to_string_lossy().into_owned();
    let mut offsets = debrief_core::skew::ClockOffsets::default();
    offsets.set(&path, -3_000).unwrap();
    store.set_clock_offsets(&offsets).unwrap();
    store.open(std::slice::from_ref(&logs.0)).unwrap().wait().unwrap();
    assert_eq!(times(&store)[0].1, Some(utc("2026-10-14 10:00:00") - 3_000));
    let summary = store.summary().unwrap();
    assert_eq!(summary.sources[0].offset_ms, -3_000);
    assert_eq!(summary.from, Some(utc("2026-10-14 10:00:00") - 3_000));

    // Replacing the set clears the offsets it leaves out and moves their events back.
    store.set_clock_offsets(&debrief_core::skew::ClockOffsets::default()).unwrap();
    assert_eq!(times(&store)[0].1, Some(utc("2026-10-14 10:00:00")));

    // A restored state brings its offsets before it reads the files.
    let state = debrief_core::api::WorkspaceState {
        roots: vec![logs.0.to_string_lossy().into_owned()],
        removed: Vec::new(),
        offsets,
    };
    store.restore(&state).unwrap().wait().unwrap();
    assert_eq!(times(&store)[0].1, Some(utc("2026-10-14 10:00:00") - 3_000));
}

#[test]
fn aligning_two_events_and_suggesting_offsets_match_the_reference_store() {
    // Two machines logging the same requests, one clock 2.5 s behind.
    let logs = TempDir::new("skew-align-logs");
    for (host, skew_ms) in [("web01", 0i64), ("web02", -2_500)] {
        std::fs::create_dir_all(logs.0.join(host)).unwrap();
        let mut text = String::new();
        for i in 0..8i64 {
            let t = 1_791_972_000_000 + i * 7_000 + skew_ms;
            text.push_str(&format!(
                "{{\"@t\":\"{}\",\"@m\":\"req {i}\",\"CorrelationId\":\"c{i}\"}}\n",
                iso(t)
            ));
        }
        std::fs::write(logs.0.join(host).join("app.clef"), text).unwrap();
    }
    let p = pair_of(Profile::default_profile(), std::slice::from_ref(&logs.0));
    let req = SuggestRequest::default();
    let (w, d) = (p.ws.suggest_offsets(&req), Skew::suggest_offsets(&p.duck, &req).unwrap());
    assert_eq!(d.fields, w.fields);
    assert_eq!(d.suggestions.len(), 1);
    assert_eq!(
        d.suggestions
            .iter()
            .map(|s| (s.suggested_ms, s.pairs, s.agreeing))
            .collect::<Vec<_>>(),
        w.suggestions
            .iter()
            .map(|s| (s.suggested_ms, s.pairs, s.agreeing))
            .collect::<Vec<_>>()
    );
    assert_eq!(d.suggestions[0].suggested_ms.abs(), 2_500);

    // Aligning the first event of web02 on the first event of web01 gives the same offset in both.
    let by_path = |paths: &std::collections::HashMap<u32, String>, host: &str| *paths.iter().find(|(_, p)| p.contains(host)).unwrap().0;
    let first_event =
        |ids: &std::collections::HashMap<u32, String>, host: &str, store_ids: &dyn Fn(u32) -> u32| store_ids(by_path(ids, host));
    let ws_ref = first_event(&p.ws_paths, "web01", &|s| p.ws.records().iter().find(|r| r.source == s).unwrap().id);
    let ws_tgt = first_event(&p.ws_paths, "web02", &|s| p.ws.records().iter().find(|r| r.source == s).unwrap().id);
    let duck_first = |host: &str| {
        let s = by_path(&p.duck_paths, host);
        let page = p
            .duck
            .page(&PageQuery {
                query: Filter {
                    sources: vec![s],
                    ..Filter::default()
                },
                limit: 1,
                ..PageQuery::default()
            })
            .unwrap();
        page.rows[0].id.0 as u32
    };
    let mut ws = p.ws;
    let wc = ws
        .set_source_offset(&SetOffsetRequest {
            spec: OffsetSpec::Align {
                reference: ws_ref,
                target: ws_tgt,
            },
        })
        .unwrap();
    let dc = p
        .duck
        .set_source_offset(&SetOffsetRequest {
            spec: OffsetSpec::Align {
                reference: duck_first("web01"),
                target: duck_first("web02"),
            },
        })
        .unwrap();
    assert_eq!(dc.offset_ms, wc.offset_ms);
    assert_eq!(dc.offset_ms, 2_500);
    // Events of one source cannot align with each other.
    let e = p
        .duck
        .set_source_offset(&SetOffsetRequest {
            spec: OffsetSpec::Align {
                reference: duck_first("web01"),
                target: duck_first("web01"),
            },
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("same source"), "{e}");
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}
