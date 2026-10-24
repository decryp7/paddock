//! The output of OS tools is parsed, never trusted: no input makes the parsers panic (conformance check C36).
use paddock_proc::{parse_probe_output, parse_pss};
use paddock_testkit::assert_never_panics;

#[test]
fn tool_output_survives_hostile_text() {
    let smaps = b"Rss: 100 kB\nPss: 90 kB\nPss_Anon: 40 kB\n";
    assert_never_panics(
        &[
            smaps,
            b"__PADDOCK_PATH_BEGIN__\n/usr/bin:/bin\n__PADDOCK_PATH_END__\n",
        ],
        |b| {
            let text = String::from_utf8_lossy(b);
            let _ = parse_pss(&text);
            let _ = parse_probe_output(&text);
        },
    );
}
