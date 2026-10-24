//! Pitwall's discovery file, API keys and engine names come from outside: no input makes the parsers panic (conformance check C36).
use paddock_pitwall::EngineChoice;
use paddock_pitwall::discovery::Discovery;
use paddock_testkit::assert_never_panics;

#[test]
fn discovery_keys_and_engine_choices_survive_hostile_text() {
    assert_never_panics(
        &[
            br#"{"version":1,"port":11435,"pid":42,"started":"2026-10-25T00:00:00Z"}"#,
            b"auto",
            b"llama.cpp",
        ],
        |b| {
            let text = String::from_utf8_lossy(b);
            let _ = Discovery::parse(&text);
            let _ = EngineChoice::parse(&text);
            let _ = paddock_pitwall::keys::parse_key(&text);
        },
    );
}
