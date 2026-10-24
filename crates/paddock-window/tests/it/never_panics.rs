//! Saved bounds and launch arguments come from disk and the OS: no input makes the parsers panic (conformance check C36).
use paddock_testkit::assert_never_panics;
use paddock_window::bounds;
use paddock_window::launch::{parse_args, parse_link};

#[test]
fn bounds_links_and_arguments_survive_hostile_text() {
    let saved = br#"{"version":1,"windows":{"main":{"x":10,"y":20,"width":800,"height":600,"maximized":false}}}"#;
    assert_never_panics(
        &[
            saved,
            b"pitwall://open?path=%2Ftmp%2Fa.log",
            b"--new-window /tmp/a.log",
        ],
        |b| {
            let text = String::from_utf8_lossy(b);
            let _ = bounds::parse(&text);
            let _ = parse_link(&text, &["pitwall"]);
            let _ = parse_args(
                text.split_whitespace().map(str::to_string),
                Some(std::path::Path::new("/work")),
                &["pitwall"],
            );
        },
    );
}
