//! The message bounds.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_channels::*;
use proptest::prelude::*;

#[test]
fn bounded_cuts_on_a_character_boundary_and_keeps_short_text() {
    let m = Message::new("k", Severity::Info, "t", "short", "s");
    assert_eq!(m.bounded(), m);
    let long = Message::new("k", Severity::Error, "t", &"€".repeat(MAX_BODY_BYTES), "s").bounded();
    assert!(long.body.len() <= MAX_BODY_BYTES && long.body.ends_with("\n[cut]"));
    assert_eq!(long.severity, Severity::Error);
    let exact = Message::new("k", Severity::Info, "t", &"a".repeat(MAX_BODY_BYTES), "s");
    assert_eq!(
        exact.bounded().body.len(),
        MAX_BODY_BYTES,
        "exactly at the limit is kept"
    );
}

#[test]
fn titles_kinds_and_sources_are_one_short_line() {
    let m = Message::new(
        &format!("k\n{}", "x".repeat(200)),
        Severity::Info,
        "a\r\nb\tc  ",
        "x",
        &"s".repeat(100),
    )
    .bounded();
    assert_eq!(m.title, "a  b c");
    assert!(!m.kind.contains('\n') && m.kind.chars().count() <= 64);
    assert_eq!(m.source.chars().count(), 64);
}

#[test]
fn severities_are_ordered() {
    assert!(Severity::Info < Severity::Warning && Severity::Warning < Severity::Error);
    assert_eq!(Severity::default(), Severity::Info);
}

proptest! {
    #[test]
    fn bounding_never_panics_and_always_fits(title in ".*", body in ".{0,300}", kind in ".*", source in ".*", mult in 0usize..400) {
        let m = Message::new(&kind, Severity::Warning, &title, &body.repeat(mult), &source).bounded();
        prop_assert!(m.title.chars().count() <= MAX_TITLE_CHARS);
        prop_assert!(m.body.len() <= MAX_BODY_BYTES);
        prop_assert!(!m.title.chars().any(char::is_control) && !m.kind.chars().any(char::is_control) && !m.source.chars().any(char::is_control));
        prop_assert_eq!(m.bounded(), m.clone(), "bounding twice changes nothing");
    }
}
