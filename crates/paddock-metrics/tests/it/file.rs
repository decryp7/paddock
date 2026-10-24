//! The store file: round trip, damaged and newer files, a changed retention.

use paddock_env::Problem;
use paddock_metrics::{MachineSample, Retention, StoreFile, TierSpec, TieredStore};
use paddock_testkit::Scratch;
use std::time::Duration;

fn retention(keep: usize) -> Retention {
    Retention::new(vec![
        TierSpec {
            bucket: Duration::from_secs(1),
            keep,
        },
        TierSpec {
            bucket: Duration::from_secs(10),
            keep,
        },
    ])
    .unwrap()
}

fn filled(r: &Retention) -> TieredStore<MachineSample> {
    let mut s = TieredStore::new(r);
    for i in 0..20u64 {
        s.push(MachineSample {
            at: i * 1000,
            cpu_permille: 5,
            ..MachineSample::default()
        });
    }
    s
}

#[test]
fn a_saved_store_comes_back() {
    let scratch = Scratch::new();
    let file = StoreFile::<MachineSample>::new(scratch.work().join("metrics.json"), retention(50));
    assert!(file.path().ends_with("metrics.json"));
    let (empty, problem) = file.load();
    assert!(
        empty.is_empty() && problem.is_none(),
        "a missing file is an empty store"
    );
    let store = filled(&retention(50));
    file.save(&store).unwrap();
    let (back, problem) = file.load();
    assert!(problem.is_none());
    assert_eq!(back, store);
}

#[test]
fn a_smaller_retention_trims_what_was_saved() {
    let scratch = Scratch::new();
    let path = scratch.work().join("metrics.json");
    StoreFile::<MachineSample>::new(&path, retention(50))
        .save(&filled(&retention(50)))
        .unwrap();
    let (back, _) = StoreFile::<MachineSample>::new(&path, retention(3)).load();
    assert!(back.len() <= retention(3).capacity());
    assert!(!back.is_empty());
}

#[test]
fn a_damaged_file_gives_an_empty_store_and_a_problem() {
    let scratch = Scratch::new();
    let path = scratch.work().join("metrics.json");
    std::fs::write(&path, "{ not json").unwrap();
    let (store, problem) = StoreFile::<MachineSample>::new(&path, retention(5)).load();
    assert!(store.is_empty());
    assert!(matches!(problem, Some(Problem::Damaged { .. })));
}

#[test]
fn a_newer_file_is_left_alone_and_saving_is_refused() {
    let scratch = Scratch::new();
    let path = scratch.work().join("metrics.json");
    std::fs::write(&path, r#"{"version": 99, "tiers": []}"#).unwrap();
    let file = StoreFile::<MachineSample>::new(&path, retention(5));
    let (store, problem) = file.load();
    assert!(store.is_empty());
    assert!(matches!(problem, Some(Problem::Newer { found: 99, .. })));
    assert!(file.save(&store).is_err());
    assert!(std::fs::read_to_string(&path).unwrap().contains("99"));
}

#[test]
fn a_file_with_another_tier_layout_starts_empty() {
    let scratch = Scratch::new();
    let path = scratch.work().join("metrics.json");
    StoreFile::<MachineSample>::new(&path, retention(50))
        .save(&filled(&retention(50)))
        .unwrap();
    let (store, problem) = StoreFile::<MachineSample>::new(&path, Retention::standard(1)).load();
    assert!(store.is_empty() && problem.is_none());
}
