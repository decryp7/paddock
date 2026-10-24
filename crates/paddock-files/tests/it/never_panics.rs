//! Paths come from the webview: no input makes the parser panic (conformance check C36).
use paddock_files::path;
use paddock_testkit::assert_never_panics;

#[test]
fn webview_paths_survive_hostile_text() {
    assert_never_panics(
        &[
            b"/Users/me/logs/app.log",
            b"relative/path",
            b"C:\\Users\\me\\a.log",
        ],
        |b| {
            let text = String::from_utf8_lossy(b);
            let _ = path::parse(&text);
        },
    );
}
