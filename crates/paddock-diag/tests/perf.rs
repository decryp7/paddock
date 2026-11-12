//! The Performance section end to end over the process-wide recorder (its own test binary: the recorder is global, and
//! `init` runs once per process): a ring left by an earlier run is read back and cleaned, recording and flushing from many
//! threads, a background flush, the bundle with no content and a JSON that survives the redactor.

use chrono::Utc;
use paddock_diag::perf::{self, Loaded, ring::Ring};
use paddock_diag::{AppInfo, Collected, Redactor, Section};
use paddock_env::{AppDirs, AppId, MapVars, Os};
use std::sync::Arc;
use std::time::Duration;

const MARKER: &str = "PLANTED-SECRET-MARKER-7f3a";

/// A name that is not `'static` in the source: what a caller that ignored the type could still forge. The filter must hold.
fn forged(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap()
}

#[test]
fn recording_to_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    // The ring an earlier run left: valid, but a hand-edited or hostile one carries content in its names.
    let at = now_ms();
    let old = format!(
        r#"{{"schema":1,"entries":[
            {{"op":"C:\\Users\\{MARKER}\\notes.txt","phase":"select {MARKER}","durationMs":99000,"atMs":{at}}},
            {{"op":"yesterday-import","phase":"scan","durationMs":88000,"atMs":{at}}}]}}"#
    );
    std::fs::write(dir.path().join(perf::RING_FILE), old).unwrap();
    perf::startup_begin();
    assert_eq!(perf::init(dir.path()), Loaded::Entries(2));
    let other = tempfile::tempdir().unwrap();
    assert_eq!(
        perf::init(other.path()),
        Loaded::Entries(2),
        "init runs once: the second call changes nothing"
    );
    perf::startup_phase("logging");
    perf::startup_phase(forged(format!("C:\\Users\\{MARKER}\\phase")));

    assert!(perf::record("import-files", Duration::from_secs(12)));
    assert!(perf::record_phase(
        "import-files",
        "parse",
        Duration::from_secs(9)
    ));
    assert!(perf::record(
        forged(format!("open {MARKER}.log")),
        Duration::from_secs(3)
    ));
    assert!(perf::record_job(
        "load-model",
        "done",
        Duration::from_secs(5)
    ));
    {
        let _t = perf::time_since(
            "page-load",
            std::time::Instant::now() - Duration::from_secs(4),
        )
        .phase("render");
    }
    // Ten threads record and flush at once: no error, nothing lost between the write and the rename, the ring stays bounded.
    std::thread::scope(|s| {
        for t in 0..10u64 {
            s.spawn(move || {
                for i in 0..2_000u64 {
                    perf::record("tick", Duration::from_millis((t + i) % 7 + 20));
                    if i % 500 == 0 {
                        perf::flush().unwrap();
                    }
                }
            });
        }
    });
    let ring_file = dir.path().join(perf::RING_FILE);
    let left = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(left, 0, "no temporary file stays");

    // A record that changed the ring is saved by a flush off the calling thread, and a new run reads it back.
    assert!(perf::record("after-the-hammer", Duration::from_secs(60)));
    perf::flush_in_background()
        .expect("no flush was queued")
        .join()
        .unwrap();
    let on_disk = Ring::parse(&std::fs::read(&ring_file).unwrap(), now_ms()).unwrap();
    assert!(
        on_disk
            .snapshot(now_ms())
            .iter()
            .any(|e| e.op == "after-the-hammer" && e.duration_ms == 60_000)
    );
    assert!(
        !perf::record("after-the-hammer", Duration::from_millis(1)),
        "too short for the full ring"
    );

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
        assert!(text.contains("import-files") && text.contains("job:load-model"));
    }
    // The bundle's own JSON files parse after the redactor that already ran (P1-1).
    for f in c
        .files()
        .iter()
        .filter(|f| f.name.ends_with(".json") && f.name.starts_with("performance"))
    {
        serde_json::from_slice::<serde_json::Value>(&f.data)
            .unwrap_or_else(|e| panic!("{}: {e}", f.name));
    }
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let slowest = v["slowest"].as_array().unwrap();
    assert_eq!(slowest.len(), perf::RING_CAPACITY, "bounded");
    assert_eq!(
        slowest[0]["op"], "invalid-name",
        "the planted 99 s entry, cleaned"
    );
    assert!(slowest.iter().any(|e| e["op"] == "yesterday-import"));
    assert!(slowest.iter().any(|e| e["op"] == "page-load"
        && e["phase"] == "render"
        && e["durationMs"].as_u64() >= Some(4000)));
    assert_eq!(v["startup"].as_array().unwrap().len(), 2);
    assert!(json.len() < 16 * 1024, "{} bytes", json.len());
    assert!(!String::from_utf8_lossy(&std::fs::read(&ring_file).unwrap()).contains(MARKER));
}
