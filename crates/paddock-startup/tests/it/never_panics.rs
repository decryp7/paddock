//! Boot errors are reported by the webview: no input makes the classifier or the cleaner panic (conformance check C36).
use paddock_startup::{BootErrorKind, clean_error};
use paddock_testkit::assert_never_panics;

#[test]
fn boot_errors_survive_hostile_text() {
    assert_never_panics(
        &[
            b"bootstrap",
            b"unhandledrejection",
            b"TypeError: x is not a function\n    at main.js:1:2",
        ],
        |b| {
            let text = String::from_utf8_lossy(b);
            let _ = BootErrorKind::parse(&text);
            let _ = clean_error(&text, 80);
        },
    );
}
