use paddock_testkit::{FakeClock, corpus, never_panics};
use std::time::Duration;

const SEED: &[u8] = b"2026-10-25 12:00:00 INFO started\nsecond line\n";

#[test]
fn the_corpus_is_deterministic_bounded_and_covers_the_hostile_shapes() {
    let c = corpus(SEED);
    assert_eq!(c, corpus(SEED));
    assert!(c.len() < 700, "{}", c.len());
    assert!(c.iter().all(|v| v.len() <= SEED.len() * 2));
    assert!(c.contains(&Vec::new()));
    assert!(c.iter().any(|v| v.len() == SEED.len() - 1), "a truncation");
    assert!(
        c.iter()
            .any(|v| v.contains(&0xc3) && std::str::from_utf8(v).is_err()),
        "invalid UTF-8"
    );
    assert!(c.iter().any(|v| v.contains(&0)), "a NUL");
    assert_eq!(
        corpus(b"").len(),
        4,
        "an empty seed still gives the fixed shapes"
    );
}

#[test]
fn a_parser_that_never_panics_passes_and_one_that_does_is_listed() {
    assert!(never_panics(&corpus(SEED), |b| std::str::from_utf8(b).is_ok()).is_empty());
    let bad = never_panics(&corpus(SEED), |b| {
        if b.contains(&0) {
            panic!("NUL");
        }
    });
    assert!(!bad.is_empty() && bad.iter().all(|v| v.contains(&0)));
}

#[test]
fn the_fake_clock_is_re_exported_and_moves_only_when_told() {
    use paddock_testkit::Clock;
    let clock = FakeClock::new();
    assert_eq!(clock.monotonic(), Duration::ZERO);
    clock.advance(Duration::from_secs(5));
    assert_eq!(clock.monotonic(), Duration::from_secs(5));
    assert_eq!(clock.wall_ms(), 5_000);
}

#[test]
fn assert_never_panics_passes_a_safe_parser_and_names_the_input_a_bad_one_panicked_on() {
    paddock_testkit::assert_never_panics(&[SEED, b"a=b"], |b| std::str::from_utf8(b).is_ok());
    let failed = std::panic::catch_unwind(|| {
        paddock_testkit::assert_never_panics(&[b"boom"], |b| {
            if b.starts_with(b"boo") && b.len() == 3 {
                panic!("bad");
            }
        })
    });
    let message = failed
        .err()
        .and_then(|e| e.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        message.contains("the parser panicked on 1 of") && message.contains("\"boo\""),
        "{message}"
    );
}
