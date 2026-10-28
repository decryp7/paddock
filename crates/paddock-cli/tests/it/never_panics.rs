//! Command-line values are user text: no input makes the parsers panic (conformance check C36).
use paddock_cli::{TableFormat, parse_size, parse_time};
use paddock_testkit::assert_never_panics;

#[test]
fn sizes_times_and_formats_survive_hostile_text() {
    assert_never_panics(
        &[
            b"10 MB",
            b"1.5GiB",
            b"2026-10-25T12:00:00Z",
            b"3 days ago",
            b"json",
        ],
        |b| {
            let text = String::from_utf8_lossy(b);
            let _ = parse_size(&text, paddock_units::Kind::Storage);
            let _ = parse_time(&text);
            let _ = TableFormat::parse(&text);
        },
    );
}
