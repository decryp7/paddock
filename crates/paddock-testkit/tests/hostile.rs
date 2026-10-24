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
        c.iter().any(|v| v.contains(&0xc3) && std::str::from_utf8(v).is_err()),
        "invalid UTF-8"
    );
    assert!(c.iter().any(|v| v.contains(&0)), "a NUL");
    assert_eq!(corpus(b"").len(), 4, "an empty seed still gives the fixed shapes");
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
