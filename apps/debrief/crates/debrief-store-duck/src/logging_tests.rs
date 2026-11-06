//! What the store logs (ER-27): one summary per import, the loops' runs and counters, the counts of an open store, and no
//! value or path in a failure's line. The records of all tests that run at the same time are in one list (see `testlog`).

use super::*;
use crate::activity::{self, Loop};
use crate::testlog;
use debrief_core::ports::{Ingest, Query};
use debrief_core::{PluginRegistry, Profile};
use std::time::Duration;

const WITHIN: Duration = Duration::from_secs(60);

/// The process's loop counters are read (and reset) by [`activity::take`]: the tests that read them take turns.
static COUNTERS: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    let _counters = COUNTERS.lock().unwrap_or_else(|p| p.into_inner());
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
    assert!(built.props.contains_key("Sources") && built.props.contains_key("Ms"));

    // The loops' turns are counted (nothing else takes the process's counters in this test binary).
    let counts = activity::take();
    let of = |l: Loop| counts.iter().find(|(x, _)| *x == l).unwrap().1;
    assert!(of(Loop::Fts).worked >= 1, "{counts:?}");
    assert!(of(Loop::Positions).worked >= 1, "an import orders its events: {counts:?}");
    let cat = of(Loop::Catalogue);
    assert!(cat.worked + cat.idle + cat.blocked >= 1, "the catalogue loop turned: {counts:?}");

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

/// The guard's wiring in both loops: a failure that lasts is one warning with the class, not one per half second; the loops
/// count their failed turns; when the cause is gone each says so once. The cause here is the `sources` table going away
/// (both loops read it), put back afterwards.
#[test]
fn a_failure_that_lasts_is_one_warning_per_loop_and_one_line_when_it_ends() {
    let _counters = COUNTERS.lock().unwrap_or_else(|p| p.into_inner());
    testlog::install();
    let dir = tempfile::tempdir().unwrap();
    let store = DuckStore::create(dir.path(), Profile::default_profile(), PluginRegistry::new()).unwrap();
    store.open(&samples()).unwrap().wait().unwrap();
    store.wait_for_index(WITHIN).unwrap();
    testlog::wait_for("store.catalogue.built", WITHIN, |_| true).expect("the catalogue is built");
    let failed_before = (
        testlog::records("store.fts.failed").len(),
        testlog::records("store.catalogue.failed").len(),
    );
    let _ = activity::take();
    let conn = store.inner.conn().unwrap();
    conn.execute_batch("ALTER TABLE sources RENAME TO sources_away").unwrap();
    // Several half-second turns fail for the same reason.

    let deadline = std::time::Instant::now() + WITHIN;
    let (mut fts_failed, mut cat_failed) = (0, 0);
    while (fts_failed < 3 || cat_failed < 3) && std::time::Instant::now() < deadline {
        for (l, c) in activity::take() {
            match l {
                Loop::Fts => fts_failed += c.failed,
                Loop::Catalogue => cat_failed += c.failed,
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        fts_failed >= 3 && cat_failed >= 3,
        "failed turns are counted: {fts_failed} / {cat_failed}"
    );
    // Repeats of a cause are counted, not logged: far fewer warnings than failed turns (a cause that changes while the table
    // goes away, as a transaction conflict does, is logged once more).
    let fts_warned = testlog::records("store.fts.failed").len() - failed_before.0;
    let cat_warned = testlog::records("store.catalogue.failed").len() - failed_before.1;
    assert!(
        fts_warned >= 1 && fts_warned * 3 <= fts_failed as usize,
        "index loop: {fts_warned} warnings for {fts_failed} failed turns"
    );
    assert!(
        cat_warned >= 1 && cat_warned * 3 <= cat_failed as usize,
        "catalogue loop: {cat_warned} warnings for {cat_failed} failed turns"
    );
    let warning = testlog::records("store.fts.failed").pop().unwrap();
    assert_eq!(warning.level, log::Level::Warn);
    assert!(
        warning.props.contains_key("Why") && !warning.message.contains("sources"),
        "{warning:?}"
    );
    // The cause goes away: each loop says once how many turns it failed.
    let recovered_before = (
        testlog::records("store.fts.recovered").len(),
        testlog::records("store.catalogue.recovered").len(),
    );
    conn.execute_batch("ALTER TABLE sources_away RENAME TO sources").unwrap();
    let deadline = std::time::Instant::now() + WITHIN;
    while (testlog::records("store.fts.recovered").len() == recovered_before.0
        || testlog::records("store.catalogue.recovered").len() == recovered_before.1)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    let fts = testlog::records("store.fts.recovered");
    let cat = testlog::records("store.catalogue.recovered");
    assert_eq!((fts.len(), cat.len()), (recovered_before.0 + 1, recovered_before.1 + 1));
    assert!(fts.last().unwrap().props["Failures"].as_u64().unwrap() >= 3);
    assert!(cat.last().unwrap().props["Failures"].as_u64().unwrap() >= 3);
}

/// A failure path with a user's names in it: files that cannot be read or parsed, in a folder with a telling name. At info and
/// above, no line of any process carries the folder, the file names or a record's text.
#[test]
fn files_that_fail_leave_no_name_or_text_in_the_log_above_debug() {
    testlog::install();
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("secret-folder");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("secret-binary.log"),
        [0xff, 0xfe, 0x00, 0x9f, b'M', b'A', b'R', b'K', 0x00, 0xc3, 0x28],
    )
    .unwrap();
    std::fs::write(
        folder.join("secret-text.log"),
        "2026-10-14 10:00:00 INFO MARKER-5521 first\n2026-10-14 10:00:01 ERROR MARKER-5521 second\n",
    )
    .unwrap();
    // A failure root cannot bypass (CI containers run as root): a link to nothing.
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path().join("nowhere"), folder.join("secret-broken.log")).unwrap();
    let store_dir = root.path().join("secret-store");
    let store = DuckStore::create(&store_dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
    let _ = store.open(std::slice::from_ref(&folder)).and_then(|s| s.wait());
    let _ = store.wait_for_index(WITHIN);
    // Our own lines are in; give the loops their turn, then look at everything logged so far.
    testlog::wait_for("store.import", WITHIN, |r| r.props.get("Outcome").is_some()).expect("the import's line");
    let above_debug: Vec<testlog::Record> = [
        "store.import",
        "store.fts.idle",
        "store.catalogue.built",
        "store.compact",
        "store.tail.read_failed",
    ]
    .iter()
    .flat_map(|e| testlog::records(e))
    .chain(testlog::all().into_iter().filter(|r| r.level <= log::Level::Info))
    .collect();
    for r in &above_debug {
        let text = format!("{r:?}");
        for secret in [
            "secret-folder",
            "secret-binary",
            "secret-text",
            "secret-broken",
            "secret-store",
            "MARKER-5521",
        ] {
            assert!(!text.contains(secret), "{secret} in a line at {:?}: {text}", r.level);
        }
    }
}

/// A contained plugin crash can quote the record the plugin was reading: the error line names the plugin and the kind, the
/// message is for debug.
#[test]
fn a_plugin_crash_names_the_plugin_at_error_and_keeps_its_message_for_debug() {
    testlog::install();
    let health = debrief_core::guard::Health::default();
    health.fail("net.test.crashing", "bad value customer-4711 in the record");
    let all = testlog::all();
    let lines: Vec<&testlog::Record> = all.iter().filter(|r| r.message.contains("net.test.crashing")).collect();
    let error = lines.iter().find(|r| r.level == log::Level::Error).expect("the error line");
    assert!(error.message.contains("crashed"), "{error:?}");
    assert!(
        !format!("{error:?}").contains("customer-4711"),
        "no record text at error: {error:?}"
    );
    assert!(
        lines
            .iter()
            .any(|r| r.level == log::Level::Debug && r.message.contains("customer-4711")),
        "the message is kept at debug: {lines:?}"
    );
}
