mod common;
use common::Collect;
use paddock_jobs::*;
use proptest::prelude::*;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn sequential_walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sequential_walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn map_equals_sequential_map(items in proptest::collection::vec(any::<u32>(), 0..200), workers in 1usize..12) {
        let got = par_map_ordered(items.clone(), Workers::new(workers), &CancelToken::new(), &(), |x| Ok(u64::from(x) * 3)).unwrap();
        prop_assert_eq!(got, items.iter().map(|x| u64::from(*x) * 3).collect::<Vec<_>>());
    }

    #[test]
    fn map_error_is_the_lowest_failing_index(items in proptest::collection::vec(0u32..50, 1..120), workers in 1usize..10) {
        let first_bad = items.iter().position(|x| x % 7 == 0);
        let r = par_map_ordered(items.clone(), Workers::new(workers), &CancelToken::new(), &(), |x| {
            if x % 7 == 0 { Err(JobError::Failed(format!("bad {x}"))) } else { Ok(x) }
        });
        match first_bad {
            None => prop_assert_eq!(r.unwrap(), items),
            Some(i) => prop_assert_eq!(r.unwrap_err().to_string(), format!("bad {}", items[i])),
        }
    }

    #[test]
    fn pipeline_delivers_in_order_for_any_window(n in 0u32..300, workers in 1usize..8, window in 0usize..20) {
        let mut got = Vec::new();
        pipeline_ordered(0..n, Workers::new(workers), window, &CancelToken::new(), Ok, |x| { got.push(x); Ok(()) }).unwrap();
        prop_assert_eq!(got, (0..n).collect::<Vec<_>>());
    }

    #[test]
    fn walk_equals_sequential_reference(shape in proptest::collection::vec((0usize..4, 0u8..6), 0..60), workers in 1usize..9) {
        let t = tempfile::tempdir().unwrap();
        let mut expected = BTreeSet::new();
        for (i, (depth, _)) in shape.iter().enumerate() {
            let mut dir = t.path().to_path_buf();
            for d in 0..*depth { dir.push(format!("d{}", (i + d) % 3)); }
            std::fs::create_dir_all(&dir).unwrap();
            let f = dir.join(format!("f{i}"));
            std::fs::write(&f, "x").unwrap();
            expected.insert(f);
        }
        let opts = WalkOptions { workers: Workers::new(workers), ..WalkOptions::default() };
        let r = par_walk(t.path(), &opts, &CancelToken::new(), &()).unwrap();
        let mut seq = Vec::new();
        sequential_walk(t.path(), &mut seq);
        seq.sort();
        let par: Vec<_> = r.entries.iter().map(|e| e.path.clone()).collect();
        prop_assert_eq!(&par, &seq);
        prop_assert_eq!(par.into_iter().collect::<BTreeSet<_>>(), expected);
    }

    #[test]
    fn hashing_is_independent_of_chunking(data in proptest::collection::vec(any::<u8>(), 0..4096), cuts in proptest::collection::vec(1usize..700, 0..12)) {
        let mut whole = HashingWriter::new(Vec::new());
        whole.write_all(&data).unwrap();
        let (_, want, _) = whole.finish().unwrap();
        let mut parts = HashingWriter::new(Vec::new());
        let mut rest = data.as_slice();
        for c in cuts { let (a, b) = rest.split_at(c.min(rest.len())); parts.write_all(a).unwrap(); rest = b; }
        parts.write_all(rest).unwrap();
        let (out, got, n) = parts.finish().unwrap();
        prop_assert_eq!(got, want);
        prop_assert_eq!((out, n), (data.clone(), data.len() as u64));
    }

    #[test]
    fn eta_never_panics_and_is_sane(steps in proptest::collection::vec((0u64..100_000, 0u64..u64::MAX / 4, proptest::option::of(0u64..u64::MAX / 4)), 0..40)) {
        let mut e = EtaEstimator::new(0);
        let mut t = 0;
        for (dt, done, total) in steps {
            t += dt;
            if let Some(ms) = e.observe(t, done, total) {
                prop_assert!(ms <= 99 * 3_600_000);
                prop_assert!(total.is_some());
            }
        }
    }

    #[test]
    fn reporter_survives_any_call_sequence(ops in proptest::collection::vec(0u8..9, 0..80), advance in proptest::collection::vec(0u64..300, 80)) {
        let sink = Collect::new();
        let clock = FakeClock::new();
        let r = Reporter::with_clock(JobId(1), JobKind::new("k"), "T", None, sink.clone(), Arc::new(clock.clone()), 100);
        for (i, op) in ops.iter().enumerate() {
            clock.advance_ms(advance[i]);
            match op {
                0 => r.start(),
                1 => r.begin_phase(Phase::items("p", "P", 10)),
                2 => r.begin_phase(Phase::discovering("d", "D")),
                3 => r.add(u64::MAX / 200),
                4 => r.set(i as u64),
                5 => r.set_total(i as u64),
                6 => r.flush(),
                7 => drop(r.uncancellable("x")),
                _ => { let _ = r.finish(JobState::Done); }
            }
        }
        let all = sink.all();
        prop_assert!(all.windows(2).all(|w| w[0].seq < w[1].seq));
        let ended = all.iter().position(|s| s.state.is_terminal());
        if let Some(i) = ended { prop_assert_eq!(i, all.len() - 1, "nothing follows the terminal event"); }
    }

    #[test]
    fn snapshot_json_round_trips(done in any::<u64>(), total in proptest::option::of(any::<u64>()), bytes in any::<bool>(), label in "\\PC{0,20}") {
        let phase = Phase { id: "p".into(), label, unit: if bytes { Unit::Bytes } else { Unit::Items }, total };
        let snap = JobSnapshot {
            id: JobId(1), kind: JobKind::new("k"), title: "t".into(), subject: None,
            state: JobState::Running { phase: Some(PhaseProgress { phase, done }) },
            seq: 1, elapsed_ms: 2, eta_ms: None, cancellable: true, not_cancellable_reason: None,
        };
        let back: JobSnapshot = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
        prop_assert_eq!(back, snap);
    }
}
