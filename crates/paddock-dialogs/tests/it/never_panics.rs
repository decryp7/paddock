//! A dialog contract file is read at start-up: no input makes the parser panic (conformance check C36).
use paddock_dialogs::contract::Contracts;
use paddock_testkit::assert_never_panics;

#[test]
fn contracts_survive_hostile_text() {
    assert_never_panics(&[br#"{"dialogs":[{"id":"a.b","commands":["x"]}]}"#], |b| {
        Contracts::parse(&String::from_utf8_lossy(b))
    });
}
