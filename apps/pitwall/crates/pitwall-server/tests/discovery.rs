//! The discovery file's `serving` flag (ADR 0009): written true on start, rewritten false (not deleted) on a clean
//! stop, tolerant of old files, and a stale pid reads as installed-not-running.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::harness_in;
use pitwall_core::discovery::{Discovery, Standing};

fn read(h: &common::Harness) -> Discovery {
    Discovery::parse(&std::fs::read_to_string(h.st.dirs.discovery()).unwrap()).unwrap()
}

#[tokio::test]
async fn start_writes_serving_true_and_a_clean_stop_rewrites_it_false() {
    let h = harness_in(tempfile::tempdir().unwrap(), false).await;
    h.st.write_discovery(40123);
    let d = read(&h);
    assert!(d.serving);
    assert_eq!(d.standing(true), Standing::Running);
    assert!(std::fs::read_to_string(h.st.dirs.discovery())
        .unwrap()
        .contains("\"serving\": true"));

    h.st.remove_discovery();
    assert!(h.st.dirs.discovery().exists(), "the file stays after a clean stop");
    let d = read(&h);
    assert!(!d.serving);
    assert_eq!(d.standing(true), Standing::InstalledNotRunning);
    // Still a valid, loopback-only file with the same identity.
    assert_eq!(d.pid, std::process::id());
}

#[tokio::test]
async fn a_stale_pid_or_an_old_file_is_judged_by_the_pid() {
    let h = harness_in(tempfile::tempdir().unwrap(), false).await;
    // A crash leaves serving true with a dead pid.
    h.st.write_discovery(40123);
    assert_eq!(read(&h).standing(false), Standing::InstalledNotRunning);
    // A file from before the field has no `serving` and reads as true.
    let old = r#"{"format":1,"version":"0.1.0","apiVersion":1,"pid":1,"port":40123,"baseUrl":"http://127.0.0.1:40123","startedAt":7}"#;
    std::fs::write(h.st.dirs.discovery(), old).unwrap();
    let d = read(&h);
    assert!(d.serving);
    assert_eq!(d.standing(true), Standing::Running);
    assert_eq!(d.standing(false), Standing::InstalledNotRunning);
}
