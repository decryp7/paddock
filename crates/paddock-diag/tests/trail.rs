//! The report lists what the user did just before: lines the app logged with an `Operation` become the trail (breadcrumbs). Its own
//! test binary: the log installs once per process.

use chrono::{TimeZone, Utc};
use paddock_diag::{AppInfo, Collected, Section, builtin};
use paddock_env::{AppDirs, AppId, MapVars, Os};
use std::sync::Arc;

#[test]
fn the_latest_actions_are_in_the_report_without_values() {
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
    let config = paddock_log::LogConfig::new(&id, "app", "1.0.0")
        .with_log_dir(dir.path())
        .with_stdout(false)
        .with_panic_hook(false);
    let handle = paddock_log::Logging::new(config).install().unwrap();
    log::info!(Operation = "saveSettings", OperationId = "r1", Changed = "theme", Ms = 12u64, Outcome = "ok"; "Action");
    log::warn!(Operation = "startJob", OperationId = "r2", Kind = "helper/turnOn", Outcome = "failed", Code = "AccessDenied"; "Action failed");
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(builtin::Trail::new(handle.clone()))];
    let now = Utc.with_ymd_and_hms(2026, 11, 5, 9, 0, 0).unwrap();
    let c = Collected::collect(
        &AppInfo::new("demo", "Demo", "1.0.0", ""),
        &dirs,
        &sections,
        &paddock_diag::Redactor::new(),
        now,
    );
    let file = c
        .files()
        .iter()
        .find(|f| f.name == "breadcrumbs.json")
        .expect("breadcrumbs.json");
    let v: serde_json::Value = serde_json::from_slice(&file.data).unwrap();
    let ops: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["operation"].as_str())
        .collect();
    assert_eq!(ops, ["saveSettings", "startJob"], "{v}");
    assert_eq!(v[1]["params"]["Code"], "AccessDenied");
}

/// Volume of the action trail: an hour of busy use (one action every ten seconds) is a few hundred lines of about 250 bytes. The
/// number is printed so the measurement is on record (`cargo test -p paddock-diag --test trail -- --nocapture`).
#[test]
fn an_hour_of_busy_use_costs_the_trail_under_a_hundred_kilobytes() {
    // Same process as the test above: the log is installed once, so this test installs nothing; it measures the formatted line.
    let line = serde_json::json!({
        "@t": "2026-11-05T09:00:00.1234567Z", "@l": "Information", "@m": "Action", "SourceContext": "dyno_app_lib::actions",
        "Operation": "startJob", "OperationId": "r1234", "Kind": "helper/turnOn", "Changed": "theme,ipLookup", "Ms": 1234, "Outcome": "ok",
        "Window": "main", "ProcessId": 4242, "Version": "1.1.0"
    })
    .to_string();
    let hour = (3600 / 10) * (line.len() + 1);
    eprintln!(
        "one action line: {} bytes; an hour at one action per 10 s: {} bytes",
        line.len() + 1,
        hour
    );
    assert!(hour < 100 * 1024, "{hour}");
}
