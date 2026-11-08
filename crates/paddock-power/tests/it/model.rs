use paddock_power::{HoldInfo, HoldRequest, MAX_TEXT_CHARS, PowerError, PowerEvent, Scope};

#[test]
fn request_text_is_cleaned_and_cut() {
    let r = HoldRequest::new(Scope::System, "  Dyno\n", "keeping\tawake\u{0}");
    assert_eq!(r.who, "Dyno");
    assert_eq!(r.why, "keeping awake");
    let long = "é".repeat(MAX_TEXT_CHARS + 50);
    assert_eq!(
        HoldRequest::new(Scope::System, "a", &long)
            .why
            .chars()
            .count(),
        MAX_TEXT_CHARS
    );
}

#[test]
fn wire_names_are_camel_case() {
    let info = HoldInfo {
        scope: Scope::SystemAndDisplay,
        why: "w".into(),
        os_ref: "r".into(),
    };
    assert_eq!(
        serde_json::to_string(&info).unwrap(),
        r#"{"scope":"systemAndDisplay","why":"w","osRef":"r"}"#
    );
    assert_eq!(
        serde_json::to_string(&PowerEvent::Woke).unwrap(),
        r#""woke""#
    );
}

#[test]
fn errors_say_what_failed() {
    assert_eq!(
        PowerError::Invalid("x".into()).to_string(),
        "invalid power request: x"
    );
    assert_eq!(
        PowerError::Unavailable {
            reason: "no D-Bus".into()
        }
        .to_string(),
        "not available here: no D-Bus"
    );
    let e = PowerError::Os {
        call: "PowerSetRequest",
        code: 5,
        detail: "denied".into(),
    };
    assert_eq!(e.to_string(), "PowerSetRequest failed with code 5: denied");
}
