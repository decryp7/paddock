//! A report's manifest is read by another app: no input makes the parser panic (conformance check C36).
use paddock_diag::Manifest;
use paddock_testkit::assert_never_panics;

#[test]
fn the_manifest_survives_hostile_bytes() {
    assert_never_panics(
        &[br#"{"kind":"paddock-diagnostics","version":1,"app":"x","files":[]}"#],
        |b| Manifest::parse(b),
    );
}
