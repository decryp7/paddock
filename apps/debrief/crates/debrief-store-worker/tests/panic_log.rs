//! The panic hook, the crash marker and the next start's line, through the worker's real log install. Its own test binary: the
//! hook and the logger are the process's, and a panic in another test must not change the marker this one reads.

use debrief_store_worker::logging::{install, log_exit, marker_path, note_previous_panic, ExitWhy};
use debrief_store_worker::protocol::{LogArgs, LogLevelArg};
use serde_json::Value;

fn lines(text: &str) -> Vec<Value> {
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[test]
fn a_panic_is_in_the_file_and_the_next_start_reports_it_without_its_message() {
    let root = tempfile::tempdir().unwrap();
    let args = LogArgs {
        dir: root.path().join("logs"),
        data_dir: Some(root.path().join("data")),
        level: LogLevelArg::Info,
        max_file_mb: 10,
        keep_files: 5,
    };
    let handle = install(&args).expect("the log installs");
    let caught = std::panic::catch_unwind(|| panic!("bad value customer-4711"));
    assert!(caught.is_err());
    let file = handle.active_file().unwrap();
    let written = lines(&std::fs::read_to_string(&file).unwrap());
    let fatal = written
        .iter()
        .find(|v| v["@l"] == "Fatal")
        .unwrap_or_else(|| panic!("no Fatal line in {written:?}"));
    assert!(fatal["Location"].as_str().unwrap().contains("panic_log.rs"), "{fatal}");
    assert_eq!(fatal["Process"], "store");
    let marker = marker_path(&args).unwrap();
    assert!(marker.exists(), "the hook leaves the marker");

    // The next start: one line with the place, the marker removed.
    note_previous_panic(&args);
    assert!(!marker.exists(), "the marker is cleared once reported");
    let after = lines(&std::fs::read_to_string(&file).unwrap());
    let reported: Vec<&Value> = after.iter().filter(|v| v["EventId"] == "store.worker.previous_panic").collect();
    assert_eq!(reported.len(), 1, "{after:?}");
    let line = reported[0];
    assert_eq!(line["@l"], "Warning");
    assert!(
        line["Location"].as_str().unwrap().contains("panic_log.rs") && line["Pid"] == std::process::id(),
        "{line}"
    );
    assert!(!line.to_string().contains("customer-4711"), "the message may name data: {line}");
    // Nothing left to report: no second line.
    note_previous_panic(&args);
    let again = lines(&std::fs::read_to_string(&file).unwrap());
    assert_eq!(again.iter().filter(|v| v["EventId"] == "store.worker.previous_panic").count(), 1);

    // The exit line is written once per process, with the first reason.
    log_exit(ExitWhy::PeerClosed, 0, None);
    log_exit(ExitWhy::StdinClosed, 0, None);
    let end = lines(&std::fs::read_to_string(&file).unwrap());
    let exits: Vec<&Value> = end.iter().filter(|v| v["EventId"] == "store.worker.exit").collect();
    assert_eq!(exits.len(), 1);
    assert_eq!(
        (exits[0]["Why"].as_str(), exits[0]["Code"].as_i64()),
        (Some("peer-closed"), Some(0))
    );
}
