//! The tiered store: folding, tiers, bounds, late samples, queries, fitting to a retention.

use paddock_metrics::{
    MAX_KEEP, MAX_POINTS, MAX_TIERS, MachineSample, Mean, Pushed, Retention, RetentionError,
    Series, TierSpec, TieredStore, bucket_start,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

fn sample(at: u64, cpu: u32, free: u64) -> MachineSample {
    MachineSample {
        at,
        cpu_permille: cpu,
        cpu_peak_permille: cpu,
        mem_used_bytes: 1000 * u64::from(cpu),
        mem_total_bytes: 10_000_000,
        disk_free_bytes: free,
        disk_total_bytes: 9_000,
        own_mem_bytes: 5,
    }
}

fn spec(bucket_ms: u64, keep: usize) -> TierSpec {
    TierSpec {
        bucket: Duration::from_millis(bucket_ms),
        keep,
    }
}

fn two_tiers() -> Retention {
    Retention::new(vec![spec(1000, 3), spec(10_000, 2)]).unwrap()
}

#[test]
fn buckets_start_on_multiples() {
    assert_eq!(bucket_start(12_345, 1000), 12_000);
    assert_eq!(bucket_start(5, 0), 5);
}

#[test]
fn mean_is_zero_without_values_and_cannot_overflow() {
    let mut m = Mean::default();
    assert_eq!((m.mean(), m.count()), (0, 0));
    m.add(u64::MAX);
    m.add(u64::MAX);
    assert_eq!(m.mean(), u64::MAX);
    assert_eq!(Mean::of(7).mean(), 7);
}

#[test]
fn samples_in_one_bucket_are_averaged_with_peak_and_minimum_free() {
    let mut s = TieredStore::new(&two_tiers());
    assert!(s.is_empty());
    assert_eq!(s.latest(), None);
    assert_eq!(s.push(sample(10_000, 100, 800)), Pushed::Stored);
    s.push(sample(10_400, 300, 500));
    s.push(sample(10_900, 200, 700));
    let p = s.latest().unwrap();
    assert_eq!(p.at, 10_000);
    assert_eq!(p.cpu_permille, 200);
    assert_eq!(p.cpu_peak_permille, 300);
    assert_eq!(p.disk_free_bytes, 500);
    assert_eq!(p.mem_used_bytes, 200_000);
    assert_eq!(p.own_mem_bytes, 5);
    assert!(!s.is_empty());
}

#[test]
fn a_peak_is_kept_when_the_sample_carries_a_higher_one_than_its_average() {
    let mut s = TieredStore::new(&two_tiers());
    let mut a = sample(0, 100, 1);
    a.cpu_peak_permille = 40; // below its own cpu: the cpu wins
    s.push(a);
    let mut b = sample(100, 100, 1);
    b.cpu_peak_permille = 900;
    s.push(b);
    assert_eq!(s.latest().unwrap().cpu_peak_permille, 900);
}

#[test]
fn every_tier_is_bounded_whatever_the_uptime() {
    let r = two_tiers();
    let mut s = TieredStore::new(&r);
    for i in 0..10_000u64 {
        s.push(sample(i * 1000, 1, 1));
        assert!(s.len() <= r.capacity());
    }
    assert_eq!(s.len(), r.capacity());
    assert_eq!(s.tier_count(), 2);
}

#[test]
fn a_query_reads_the_finest_tier_that_covers_the_range() {
    let r = two_tiers(); // 1 s x 3 = 3 s, then 10 s x 2 = 20 s
    let mut s = TieredStore::new(&r);
    for i in 0..30u64 {
        s.push(sample(i * 1000, 1, 1));
    }
    assert_eq!(s.tier_for(Duration::from_secs(2)), 0);
    assert_eq!(s.tier_for(Duration::from_secs(15)), 1);
    assert_eq!(
        s.tier_for(Duration::from_secs(900)),
        1,
        "the coarsest when none covers it"
    );
    assert_eq!(r.tier_for(Duration::from_secs(2)), 0);
    assert_eq!(r.tier_for(Duration::from_secs(900)), 1);
    let fine = s.query(29_000, Duration::from_secs(2));
    assert_eq!(
        fine.iter().map(|p| p.at).collect::<Vec<_>>(),
        vec![27_000, 28_000, 29_000]
    );
    let coarse = s.query(29_000, Duration::from_secs(15));
    assert_eq!(
        coarse.iter().map(|p| p.at).collect::<Vec<_>>(),
        vec![20_000]
    );
}

#[test]
fn a_sample_from_the_past_is_dropped_not_reordered() {
    let mut s = TieredStore::new(&two_tiers());
    s.push(sample(50_000, 1, 1));
    assert_eq!(s.push(sample(1_000, 9, 9)), Pushed::TooLate);
    assert_eq!(s.latest().unwrap().cpu_permille, 1);
    // Older than the fine bucket but inside the coarse one: still stored by that tier.
    assert_eq!(s.push(sample(49_000, 3, 1)), Pushed::TooLate);
    s.push(sample(55_000, 1, 1));
    assert_eq!(s.push(sample(52_000, 1, 1)), Pushed::Stored);
}

#[test]
fn an_empty_store_answers_with_nothing() {
    let s: TieredStore<MachineSample> = TieredStore::default();
    assert!(s.query(1_000_000, Duration::from_secs(60)).is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(s.tier_count(), 4);
}

#[test]
fn fitting_applies_a_new_keep_and_resets_a_different_layout() {
    let mut s = TieredStore::new(&two_tiers());
    for i in 0..10u64 {
        s.push(sample(i * 1000, 1, 1));
    }
    let fewer = Retention::new(vec![spec(1000, 1), spec(10_000, 1)]).unwrap();
    let fitted = s.clone().fit(&fewer);
    assert!(fitted.len() <= fewer.capacity());
    assert_eq!(fitted.latest(), s.latest());
    let other = Retention::new(vec![spec(2000, 3), spec(10_000, 2)]).unwrap();
    assert!(s.clone().fit(&other).is_empty());
    let three = Retention::standard(1);
    assert!(s.fit(&three).is_empty());
}

#[test]
fn the_store_round_trips_through_json() {
    let mut s = TieredStore::new(&two_tiers());
    for i in 0..7u64 {
        s.push(sample(i * 1000, 10 * i as u32, 100 - i));
    }
    let json = serde_json::to_string(&s).unwrap();
    let back: TieredStore<MachineSample> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn presets_are_valid_and_bounded() {
    for r in [
        Retention::standard(0),
        Retention::standard(30),
        Retention::standard(9999),
        Retention::sparse(90),
        Retention::default(),
    ] {
        assert!(Retention::new(r.tiers().to_vec()).is_ok(), "{r:?}");
        assert!(r.capacity() <= MAX_POINTS);
    }
    assert_eq!(
        Retention::standard(0).tiers()[3].keep,
        24,
        "days clamp to 1..=365"
    );
    assert_eq!(Retention::standard(9999).tiers()[3].keep, 365 * 24);
    assert_eq!(
        Retention::sparse(90).tiers()[0].bucket,
        Duration::from_secs(30)
    );
    assert_eq!(
        Retention::standard(1).tiers()[0].span(),
        Duration::from_secs(3600)
    );
    assert_eq!(
        TierSpec {
            bucket: Duration::from_secs(1),
            keep: usize::MAX
        }
        .span(),
        Duration::from_secs(u64::from(u32::MAX))
    );
}

#[test]
fn bad_retentions_are_refused_with_a_reason() {
    assert_eq!(Retention::new(vec![]), Err(RetentionError::Empty));
    let many: Vec<TierSpec> = (1..=MAX_TIERS as u64 + 1)
        .map(|i| spec(i * 1000, 1))
        .collect();
    assert_eq!(Retention::new(many), Err(RetentionError::TooManyTiers));
    assert_eq!(
        Retention::new(vec![spec(0, 1)]),
        Err(RetentionError::ZeroBucket)
    );
    assert_eq!(
        Retention::new(vec![TierSpec {
            bucket: Duration::from_micros(10),
            keep: 1
        }]),
        Err(RetentionError::ZeroBucket)
    );
    assert_eq!(
        Retention::new(vec![spec(1000, 0)]),
        Err(RetentionError::BadKeep)
    );
    assert_eq!(
        Retention::new(vec![spec(1000, MAX_KEEP + 1)]),
        Err(RetentionError::BadKeep)
    );
    assert_eq!(
        Retention::new(vec![spec(1000, 1), spec(1000, 1)]),
        Err(RetentionError::NotIncreasing)
    );
    assert_eq!(
        Retention::new(vec![
            spec(1000, MAX_KEEP),
            spec(2000, MAX_KEEP),
            spec(3000, 1)
        ]),
        Err(RetentionError::TooManyPoints)
    );
    for e in [
        RetentionError::Empty,
        RetentionError::TooManyTiers,
        RetentionError::ZeroBucket,
        RetentionError::BadKeep,
        RetentionError::NotIncreasing,
        RetentionError::TooManyPoints,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

/// A series with a non-Copy bucket (Pitwall keeps maps and histograms): the trait does not require Copy.
#[derive(Clone, Serialize, Deserialize)]
struct Counted {
    at: u64,
    labels: Vec<String>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
struct CountedAcc {
    start: u64,
    labels: Vec<String>,
}

impl Series for Counted {
    type Acc = CountedAcc;
    fn at(&self) -> u64 {
        self.at
    }
    fn begin(start: u64, s: &Self) -> CountedAcc {
        CountedAcc {
            start,
            labels: s.labels.clone(),
        }
    }
    fn absorb(acc: &mut CountedAcc, s: &Self) {
        acc.labels.extend(s.labels.iter().cloned());
    }
    fn point(acc: &CountedAcc) -> Self {
        Counted {
            at: acc.start,
            labels: acc.labels.clone(),
        }
    }
    fn start(acc: &CountedAcc) -> u64 {
        acc.start
    }
}

#[test]
fn a_series_may_keep_a_non_copy_bucket() {
    let mut s = TieredStore::new(&two_tiers());
    s.push(Counted {
        at: 0,
        labels: vec!["a".into()],
    });
    s.push(Counted {
        at: 100,
        labels: vec!["b".into()],
    });
    assert_eq!(s.latest().unwrap().labels, vec!["a", "b"]);
}
