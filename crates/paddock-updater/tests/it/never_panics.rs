//! The release manifest comes from the network: no input makes the parser panic (conformance check C34).
use paddock_testkit::{corpus, never_panics};

const SEED: &str = r#"{"version":"1.2.3","notes":"n","pub_date":"2026-10-25T00:00:00Z","platforms":{"darwin-aarch64":{"signature":"s","url":"https://example.org/a.tar.gz"}}}"#;

#[test]
fn the_manifest_parser_survives_hostile_input() {
    let inputs = corpus(SEED.as_bytes());
    let panicked = never_panics(&inputs, |bytes| {
        paddock_updater::manifest::parse(&String::from_utf8_lossy(bytes))
    });
    assert!(
        panicked.is_empty(),
        "panicked on {} input(s), first: {:?}",
        panicked.len(),
        panicked
            .first()
            .map(|b| String::from_utf8_lossy(b).into_owned())
    );
}
