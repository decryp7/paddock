use paddock_jobs::EtaEstimator;

#[test]
fn no_eta_for_unknown_or_zero_total() {
    let mut e = EtaEstimator::new(0);
    assert_eq!(e.observe(5_000, 100, None), None);
    assert_eq!(e.observe(6_000, 200, Some(0)), None);
}

#[test]
fn no_eta_until_two_seconds_and_three_percent() {
    let mut e = EtaEstimator::new(0);
    assert_eq!(e.observe(1_000, 500, Some(1000)), None, "too early");
    let mut e = EtaEstimator::new(0);
    assert_eq!(e.observe(3_000, 1, Some(1000)), None, "too little done");
}

#[test]
fn steady_rate_gives_exact_eta_and_done_gives_zero() {
    let mut e = EtaEstimator::new(0);
    for t in 1..=3 {
        e.observe(t * 1000, t * 100, Some(1000));
    }
    assert_eq!(e.observe(3_000, 300, Some(1000)), Some(7_000));
    assert_eq!(e.observe(10_000, 1000, Some(1000)), Some(0));
    assert_eq!(
        e.observe(11_000, 5000, Some(1000)),
        Some(0),
        "done past total clamps"
    );
}

#[test]
fn smoothing_damps_a_slow_second_and_stalls_give_none() {
    let mut e = EtaEstimator::new(0);
    e.observe(2_000, 200, Some(1000));
    let steady = e.observe(3_000, 300, Some(1000)).unwrap();
    let slow = e.observe(4_000, 310, Some(1000)).unwrap();
    assert!(
        slow > steady && slow < steady * 8,
        "smoothed, not a jump: {steady} -> {slow}"
    );
    // No movement at all for a long time: rate stays, time passes.
    assert!(e.observe(5_000, 310, Some(1000)).is_some());
    // Never any progress: no rate, no estimate.
    let mut z = EtaEstimator::new(0);
    assert_eq!(z.observe(5_000, 0, Some(1000)), None);
}

#[test]
fn restart_of_the_scale_resets() {
    let mut e = EtaEstimator::new(0);
    e.observe(3_000, 600, Some(1000));
    assert_eq!(e.observe(3_100, 5, Some(1000)), None);
}

#[test]
fn absurd_estimates_are_hidden() {
    let mut e = EtaEstimator::new(0);
    e.observe(2_000, 1, Some(u64::MAX / 2));
    e.observe(4_000, 2, Some(u64::MAX / 2));
    assert_eq!(e.observe(4_000, 2, Some(u64::MAX / 2)), None);
}
