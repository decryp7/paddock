//! The recurring class (a shared redactor reshaping data a section contributes: the JSON redactor cut a long list, the text
//! redactor masked a 13-digit number as a card): every built-in section and the performance section, over realistic data, and
//! every `.json` file of the report, as written by `Collected::collect` with `Redactor::new()` (every file passes it there),
//! still parses.

use chrono::{TimeZone, Utc};
use paddock_diag::perf::PerformanceSection;
use paddock_diag::*;
use paddock_env::{AppDirs, AppId, MapVars, Os};
use std::sync::Arc;

#[test]
fn every_json_file_of_every_builtin_section_parses_after_the_redactor() {
    let dir = tempfile::tempdir().unwrap();
    let id = AppId::new("demo", "Demo").unwrap();
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", dir.path()),
        None,
        true,
    )
    .unwrap();
    // Settings with the numbers that broke text redaction: 13-digit times, terabyte sizes, a card-shaped value.
    std::fs::create_dir_all(dirs.config()).unwrap();
    let settings = serde_json::json!({
        "lastRunMs": 1_794_361_422_684u64, "cacheBytes": 1_099_511_627_776u64, "ids": [4_111_111_111_111_111u64, 79927398713u64],
        "nested": { "list": (0..300).map(|i| serde_json::json!({"at": 1_700_000_000_000u64 + i, "n": i})).collect::<Vec<_>>() }
    });
    std::fs::write(
        dirs.config().join(paddock_settings::FILE_NAME),
        settings.to_string(),
    )
    .unwrap();
    paddock_diag::perf::record("zz-shape-check", std::time::Duration::from_secs(3));
    let mut sections = builtin::all();
    sections.push(Arc::new(PerformanceSection::new()));
    let now = Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap();
    let collected = Collected::collect(
        &AppInfo::new("demo", "Demo", "1.0.0", ""),
        &dirs,
        &sections,
        &Redactor::new(),
        now,
    );
    let jsons: Vec<_> = collected
        .files()
        .iter()
        .filter(|f| f.name.ends_with(".json"))
        .collect();
    let names: Vec<&str> = jsons.iter().map(|f| f.name.as_str()).collect();
    for want in ["system-info.json", "performance.json"] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    for f in jsons {
        serde_json::from_slice::<serde_json::Value>(&f.data)
            .unwrap_or_else(|e| panic!("{} was written broken: {e}", f.name));
    }
}
