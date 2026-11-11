//! The Performance section end to end over the process-wide recorder (its own test binary: the recorder is global): record,
//! flush, a new run reads the ring back, a damaged file is replaced, and the report holds no content.

use chrono::Utc;
use paddock_diag::perf::{self, Loaded};
use paddock_diag::{AppInfo, Collected, Redactor, Section};
use paddock_env::{AppDirs, AppId, MapVars, Os};
use std::sync::Arc;
use std::time::Duration;

const MARKER: &str = "PLANTED-SECRET-MARKER-7f3a";

#[test]
fn recording_to_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    // A ring file damaged by a crash: the app starts anyway.
    std::fs::write(
        dir.path().join(perf::RING_FILE),
        b"{ \"schema\": 1, \"entries\": [ {",
    )
    .unwrap();
    perf::startup_begin();
    assert!(matches!(perf::init(dir.path()), Loaded::Unusable(_)));
    perf::startup_phase("logging");
    perf::startup_phase(&format!("C:\\Users\\{MARKER}\\phase"));

    perf::record("import-files", Duration::from_secs(12));
    perf::record_phase("import-files", "parse", Duration::from_secs(9));
    perf::record(&format!("open {MARKER}.log"), Duration::from_secs(3));
    perf::record_phase("query", &format!("select {MARKER}"), Duration::from_secs(2));
    {
        let _t = perf::time_since(
            "page-load",
            std::time::Instant::now() - Duration::from_secs(4),
        )
        .phase("render");
    }
    // 10 000 short operations never grow the ring past its bound.
    for i in 0..10_000u64 {
        perf::record("tick", Duration::from_millis(i % 7 + 20));
    }

    let id = AppId::new("demo", "Demo").unwrap();
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", dir.path()),
        None,
        true,
    )
    .unwrap();
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(perf::PerformanceSection::new())];
    let c = Collected::collect(
        &AppInfo::new("demo", "Demo", "1.0.0", ""),
        &dirs,
        &sections,
        &Redactor::new(),
        Utc::now(),
    );
    assert!(c.notes().is_empty(), "{:?}", c.notes());

    let get = |name: &str| {
        String::from_utf8(
            c.files()
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .data
                .clone(),
        )
        .unwrap()
    };
    let (json, summary) = (get("performance.json"), get("performance-summary.txt"));
    for text in [&json, &summary] {
        assert!(!text.contains(MARKER), "content leaked:\n{text}");
        assert!(text.contains(perf::INVALID_NAME));
        assert!(text.contains("import-files"));
    }
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let slowest = v["slowest"].as_array().unwrap_or_else(|| panic!("{json}"));
    assert_eq!(slowest.len(), perf::RING_CAPACITY, "bounded");
    assert_eq!(slowest[0]["op"], "import-files");
    assert!(slowest.iter().any(|e| e["op"] == "page-load"
        && e["phase"] == "render"
        && e["durationMs"].as_u64() >= Some(4000)));
    assert_eq!(v["startup"].as_array().unwrap().len(), 2);
    assert!(json.len() < 16 * 1024, "{} bytes", json.len());

    // The flush replaced the damaged file; a later run reads the ring back.
    let ring = std::fs::read(dir.path().join(perf::RING_FILE)).unwrap();
    assert!(ring.len() < perf::ring::MAX_RING_FILE as usize);
    assert!(!String::from_utf8_lossy(&ring).contains(MARKER));
    assert_eq!(perf::init(dir.path()), Loaded::Entries(perf::RING_CAPACITY));
}
