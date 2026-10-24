//! The log level setting and the crash marker are read from disk: no input makes the readers panic (conformance check C36).
use paddock_log::{CrashMarker, LogLevel};
use paddock_testkit::{Scratch, assert_never_panics};

#[test]
fn levels_and_the_crash_marker_survive_hostile_bytes() {
    let scratch = Scratch::new();
    let marker = scratch.work().join("crash.json");
    assert_never_panics(
        &[
            br#"{"at":1,"message":"boom","backtrace":["a","b"]}"#,
            b"debug",
        ],
        |b| {
            let _ = LogLevel::parse(&String::from_utf8_lossy(b));
            std::fs::write(&marker, b).expect("write the marker");
            let _ = CrashMarker::read(&marker);
        },
    );
    scratch.finish().expect("nothing left in tmp");
}
