//! The runtime catalogue is downloaded and platform keys come from files: no input makes the parsers panic (conformance check C36).
use paddock_runtime::{Catalogue, Platform};
use paddock_testkit::assert_never_panics;

#[test]
fn catalogues_and_platform_keys_survive_hostile_text() {
    let catalogue = br#"{"version":1,"runtimes":[{"id":"llama","version":"1","platforms":{"darwin-aarch64":{"url":"https://example.org/a.zip","sha256":"00"}}}]}"#;
    assert_never_panics(&[catalogue, b"darwin-aarch64", b"windows-x86_64"], |b| {
        let text = String::from_utf8_lossy(b);
        let _ = Catalogue::parse(&text);
        let _ = Platform::parse(&text);
    });
}
