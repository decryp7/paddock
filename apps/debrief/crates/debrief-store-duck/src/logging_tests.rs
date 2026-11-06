//! What the store logs (ER-27): one summary per import, the loops' runs and counters, the counts of an open store, and no
//! value or path in a failure's line. The records of all tests that run at the same time are in one list (see `testlog`).

use super::*;
use crate::activity::{self, Loop};
use crate::testlog;
use debrief_core::ports::{Ingest, Query};
use debrief_core::{PluginRegistry, Profile};
use std::time::Duration;

const WITHIN: Duration = Duration::from_secs(60);

fn samples() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    vec![root.join("serilog-text"), root.join("serilog-clef")]
}

fn prop(r: &testlog::Record, key: &str) -> u64 {
    r.props
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("{key} is not a number in {r:?}"))
}

#[test]
fn an_import_logs_one_summary_the_loops_log_their_runs_and_the_open_store_counts() {
    testlog::install();
    let dir = tempfile::tempdir().unwrap();
    let store = DuckStore::create(dir.path(), Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    let total = store.summary().unwrap().total as u64;
    let sources = store
        .inner
        .conn()
        .unwrap()
        .query_row("SELECT count(*) FROM sources", [], |r| r.get::<_, u64>(0))
        .unwrap();

    // One summary per import: outcome, counts and the time of each stage.
    let import = testlog::wait_for("store.import", WITHIN, |r| {
        r.props.get("Outcome").and_then(serde_json::Value::as_str) == Some("done")
            && r.props.get("Events").and_then(serde_json::Value::as_u64) == Some(total)
    })
    .expect("the import's summary line");
    assert_eq!(import.level, log::Level::Info);
    assert_eq!(prop(&import, "Sources"), sources);
    assert!(prop(&import, "Bytes") > 0);
    for key in [
        "ParseMs",
        "FinishMs",
        "MaintainMs",
        "CompactMs",
        "PosMs",
        "SegmentsReplaced",
        "SegmentsWritten",
        "Ms",
    ] {
        assert!(import.props.contains_key(key), "{key} is in {import:?}");
    }
    assert!(prop(&import, "Ms") >= prop(&import, "ParseMs"));
    assert_eq!(prop(&import, "MaintainMs"), prop(&import, "CompactMs") + prop(&import, "PosMs"));

    // The index loop reports a run when it goes idle: steps, segments and documents; the catalogue loop what it built.
    let status = store.wait_for_index(WITHIN).unwrap();
    assert!(status.ready);
    let idle = testlog::wait_for("store.fts.idle", WITHIN, |r| {
        r.props.get("Documents").and_then(serde_json::Value::as_u64).unwrap_or(0) > 0
    })
    .expect("the index run's idle line");
    assert!(prop(&idle, "Steps") >= 1 && prop(&idle, "Segments") >= 1);
    assert!(prop(&idle, "BusyMs") <= prop(&idle, "Ms"));
    for key in ["WakesImport", "WakesRetry", "WakesBackstop"] {
        assert!(idle.props.contains_key(key), "{key} is in {idle:?}");
    }
    let built = testlog::wait_for("store.catalogue.built", WITHIN, |r| prop(r, "Rows") > 0).expect("the catalogue's build line");
    assert!(built.props.contains_key("Source") && built.props.contains_key("Ms"));

    // The loops' turns are counted (nothing else takes the process's counters in this test binary).
    let counts = activity::take();
    let of = |l: Loop| counts.iter().find(|(x, _)| *x == l).unwrap().1;
    assert!(of(Loop::Fts).worked >= 1, "{counts:?}");
    assert!(of(Loop::Positions).worked >= 1, "an import orders its events: {counts:?}");
    assert!(of(Loop::Catalogue).worked >= 1, "{counts:?}");

    // The open store's counts for the worker's open line.
    let open = store.open_counts().unwrap();
    assert!(open.segments >= 1 && open.waiting == 0 && open.data_version >= 1, "{open:?}");
}

#[test]
fn an_import_that_failed_logs_its_class_at_warn_and_the_text_only_at_debug() {
    testlog::install();
    let quoted = StoreError::Engine("Conversion Error: Could not convert string 'customer-4711' to INT32".into());
    ImportStats::default().log(&Err(quoted), false, Duration::from_millis(5));
    let failed = testlog::wait_for("store.import", Duration::from_secs(5), |r| {
        r.props.get("Outcome").and_then(serde_json::Value::as_str) == Some("failed")
    })
    .expect("the failure's line");
    assert_eq!(failed.level, log::Level::Warn);
    assert_eq!(failed.props["Why"], "store: Conversion Error");
    assert!(!format!("{failed:?}").contains("customer-4711"), "no value at warn: {failed:?}");
    let detail = testlog::records("store.import.error").pop().expect("the debug line");
    assert_eq!(detail.level, log::Level::Debug);
    assert!(detail.message.contains("customer-4711"));

    ImportStats::default().log(&Ok(ImportReport::default()), true, Duration::from_millis(1));
    let cancelled = testlog::wait_for("store.import", Duration::from_secs(5), |r| {
        r.props.get("Outcome").and_then(serde_json::Value::as_str) == Some("cancelled")
    })
    .expect("the cancelled line");
    assert_eq!(cancelled.level, log::Level::Info);
}

#[test]
fn milliseconds_are_whole_and_saturate() {
    assert_eq!(ms(Duration::from_millis(1500)), 1500);
    assert_eq!(ms(Duration::ZERO), 0);
    assert_eq!(ms(Duration::MAX), u64::MAX);
}
