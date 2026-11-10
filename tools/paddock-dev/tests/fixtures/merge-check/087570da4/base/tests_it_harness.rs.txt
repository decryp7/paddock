//! The harness against its misbehaving fixture: conformance passes for a well-behaved plugin, a panic is a typed
//! error and not a hang, a hang is cut off, and a contained plugin cannot read the user's files.

use debrief_ipc::peer::code;
use debrief_plugin_test::Launch;
use debrief_plugin_test::PluginUnderTest;
use std::time::Duration;

const FIXTURE: &str = env!("CARGO_BIN_EXE_debrief-plugin-test-fixture");

#[test]
fn a_well_behaved_plugin_passes_conformance_and_imports() {
    let p = PluginUnderTest::launch(FIXTURE).unwrap();
    assert_eq!(p.manifest().id.as_ref(), "net.decryptology.test-fixture");
    let report = p.conformance();
    report.assert_passed();
    assert_eq!(report.checks.len(), 6, "{report}");
    let out = p.import("fixture", "a.log", b"one\ntwo\n").unwrap();
    assert_eq!(out.records.iter().map(|r| r.message.as_str()).collect::<Vec<_>>(), ["one", "two"]);
    assert_eq!(p.sniff("fixture", "a.log", b"#fixture\n").unwrap(), 100);
    assert_eq!(p.invoke("fixture.tool", "echo", serde_json::json!({"a": 1})).unwrap()["a"], 1);
    assert!(p.invoke("fixture.tool", "nope", serde_json::json!({})).is_err());
}

#[test]
fn a_panic_is_a_typed_error_and_the_plugin_keeps_serving() {
    let p = PluginUnderTest::launch(FIXTURE).unwrap();
    let e = p.import("fixture", "a.log", b"panic\n").unwrap_err();
    assert!(e.to_string().contains("panicked"), "{e}");
    let raw = p.request_raw(debrief_ipc::wire::encode(&debrief_plugin_api::protocol::PluginRequest::Manifest).unwrap());
    assert!(raw.is_ok() && p.alive());
    let e = p.import("fixture", "a.log", b"fail\n").unwrap_err();
    assert!(e.to_string().contains("on purpose"), "{e}");
    match p.request_raw(vec![0xff; 4]) {
        Err(debrief_ipc::IpcError::Remote(e)) => assert_eq!(e.code, code::INVALID),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_hung_import_is_cut_off_and_the_conformance_checks_report_it() {
    let p = PluginUnderTest::launch(FIXTURE).unwrap();
    // The verdict is that a plugin which never answers is given up on, whatever the timeout; no elapsed time is asserted,
    // the message names the timeout the call was given.
    let e = p.import_within("fixture", "a.log", b"hang\n", Duration::from_secs(1)).unwrap_err();
    assert!(e.to_string().contains("did not answer within 1 s"), "{e}");
    let hang = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(hang.path(), b"hang\n").unwrap();
    // The whole set against the hung plugin ends with a report (what `cargo debrief-plugin` relies on); the import
    // timeout is the one wall-clock wait in it, so the test shortens it.
    let fresh = PluginUnderTest::launch_with(
        FIXTURE,
        Launch {
            import_timeout: Duration::from_secs(1),
            ..Launch::default()
        },
    )
    .unwrap();
    let report = fresh.conformance_with(&[hang.path().to_path_buf()]);
    let by_name = |n: &str| {
        report
            .checks
            .iter()
            .find(|c| c.name == n)
            .unwrap_or_else(|| panic!("no check {n}: {report}"))
    };
    assert_eq!(report.checks.len(), 6, "{report}");
    let bounded = by_name("bounded-output");
    assert!(!bounded.passed && bounded.detail.contains("did not answer within 1 s"), "{report}");
    // The bounded-output check before it waited for the hang too; cancellation is what the importer's ignoring of it fails.
    let cancel = by_name("cancellation");
    assert!(!cancel.passed && cancel.detail.contains("kept running"), "{report}");
    assert!(by_name("manifest").passed && by_name("typed-errors").passed, "{report}");
    // On the small-frame connection the plugin's read of its 2 MiB input is refused before the importer runs, so the
    // hang is never reached: a typed error, whatever the timing (a hang would be judged on the "did not answer" error).
    let oversized = by_name("oversized-frames");
    assert!(
        oversized.passed && oversized.detail.contains("refused with a typed error"),
        "{report}"
    );
}

#[test]
fn a_read_that_stops_advancing_fails_the_cancellation_check_with_what_was_read() {
    let fresh = PluginUnderTest::launch_with(
        FIXTURE,
        Launch {
            import_timeout: Duration::from_secs(1),
            read_stall: Duration::from_secs(1),
            // The host stops answering the plugin's reads after 1 MiB of the 16 MiB the check lends.
            stall_reads_after: Some(1 << 20),
            ..Launch::default()
        },
    )
    .unwrap();
    let report = fresh.conformance();
    let cancel = report.checks.iter().find(|c| c.name == "cancellation").unwrap();
    assert!(!cancel.passed && cancel.detail.contains("made no progress"), "{report}");
}

#[cfg(windows)]
#[test]
fn a_contained_plugin_cannot_read_the_users_files_unless_run_as_first_party() {
    let secret = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(secret.path(), b"secret").unwrap();
    let args = serde_json::json!({ "path": secret.path() });
    let contained = PluginUnderTest::launch(FIXTURE).unwrap();
    let r = contained.invoke("fixture.tool", "read", args.clone()).unwrap();
    assert!(r.get("denied").is_some(), "{r}");
    // Its own data folder is its to use.
    std::fs::write(contained.data_dir().join("mine.txt"), b"ok").unwrap();
    let own = serde_json::json!({ "path": contained.data_dir().join("mine.txt") });
    assert_eq!(contained.invoke("fixture.tool", "read", own).unwrap()["read"], 2);
    let first_party = PluginUnderTest::launch_with(
        FIXTURE,
        Launch {
            third_party: false,
            ..Launch::default()
        },
    )
    .unwrap();
    // Debrief's own plugins run at low integrity, which reads but cannot write.
    assert_eq!(first_party.invoke("fixture.tool", "read", args).unwrap()["read"], 6);
    assert_eq!(contained.granted(), [debrief_plugin_api::Permission::Network]);
}
