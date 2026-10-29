//! A3's changes to the index: live phases, the pool, the stamp shortcut, quarantine and cancel in every phase.

use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

use pacenotes_core::ports::Store;
use pacenotes_index::{Denylist, EpochClock, IndexError, IndexStore, Indexer, Progress};
use paddock_jobs::{CancelToken, Workers};

use crate::support::{ParseSource, RecordingFs, p, workspace};

/// Records what the job would show.
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<String>>,
    on_phase: Option<(&'static str, CancelToken)>,
}

impl Progress for Recorder {
    fn phase(&self, id: &'static str, label: &'static str, total: Option<u64>) {
        self.events.lock().unwrap().push(format!("phase {id} {label:?} total={total:?}"));
        if let Some((at, token)) = &self.on_phase
            && *at == id
        {
            token.cancel();
        }
    }

    fn add(&self, n: u64) {
        self.events.lock().unwrap().push(format!("add {n}"));
    }

    fn set_total(&self, total: u64) {
        self.events.lock().unwrap().push(format!("total {total}"));
    }
}

/// A clock the test sets.
struct SetClock(AtomicI64);

impl EpochClock for SetClock {
    fn now_ns(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn many(n: usize) -> Vec<(String, String)> {
    (0..n)
        .map(|i| {
            (
                format!("src/m{i:03}.rs"),
                format!("pub fn f{i}() {{}}\n\npub fn g{i}() {{ f{i}(); }}\n"),
            )
        })
        .collect()
}

fn workspace_of(files: &[(String, String)]) -> paddock_testkit::Scratch {
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    workspace(&refs)
}

#[test]
fn a_run_reports_four_phases_in_order_each_on_its_own_scale_and_never_backwards() {
    let ws = workspace_of(&many(12));
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let (source, deny, rec) = (ParseSource::default(), Denylist::new(), Recorder::default());
    let ix = Indexer {
        progress: &rec,
        ..Indexer::new(&fs, &source, &deny)
    };
    ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    let events = rec.events.lock().unwrap().clone();
    let phases: Vec<_> = events.iter().filter(|e| e.starts_with("phase")).cloned().collect();
    assert_eq!(
        phases,
        [
            "phase walk \"Finding files\" total=None",
            "phase read \"Reading files\" total=Some(12)",
            "phase extract \"Reading code\" total=Some(12)",
            "phase save \"Saving the index\" total=None",
        ]
    );
    // the walk learns its total at the end; each counting phase adds exactly its total, and nothing is subtracted
    let mut sums = Vec::new();
    let mut current = 0u64;
    for e in &events {
        if e.starts_with("phase") {
            sums.push(current);
            current = 0;
        } else if let Some(n) = e.strip_prefix("add ") {
            current += n.parse::<u64>().unwrap();
        }
    }
    sums.push(current);
    assert_eq!(
        sums,
        [0, 13, 12, 12, 0],
        "walk counts entries (files and the src folder), read and extract count files, save counts nothing: {sums:?}"
    );
    assert!(events.contains(&"total 12".to_owned()));
}

#[test]
fn the_pool_gives_the_same_index_whatever_the_thread_count() {
    let files = many(60);
    let mut dumps = Vec::new();
    for (read, parse) in [(1, 1), (4, 4), (3, 8)] {
        let ws = workspace_of(&files);
        let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
        let fs = RecordingFs::new(&ws.path("ws"));
        let (source, deny) = (ParseSource::default(), Denylist::new());
        let ix = Indexer {
            read_workers: Workers::new(read),
            parse_workers: Workers::new(parse),
            ..Indexer::new(&fs, &source, &deny)
        };
        let report = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
        assert_eq!(report.added, 60);
        let ids: Vec<_> = (0..60)
            .map(|i| opened.store.symbol_ids(&p(&format!("src/m{i:03}.rs"))).unwrap())
            .collect();
        dumps.push((opened.store.dump_text().unwrap(), ids));
    }
    assert!(dumps.iter().all(|d| d == &dumps[0]), "thread counts change nothing but the time");
}

#[test]
fn a_file_whose_size_and_mtime_match_is_not_read_again_unless_it_is_racy() {
    let ws = workspace_of(&many(5));
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let (source, deny) = (ParseSource::default(), Denylist::new());
    let far_future = 4_000_000_000_000_000_000_i64;
    let clock = SetClock(AtomicI64::new(far_future));
    let run = |store: &mut IndexStore| {
        let fs = RecordingFs::new(&ws.path("ws"));
        let ix = Indexer {
            clock: &clock,
            ..Indexer::new(&fs, &source, &deny)
        };
        let report = ix.run(store, &CancelToken::new()).unwrap();
        (report, fs.reads().len())
    };
    let (first, reads) = run(&mut opened.store);
    assert_eq!(
        (first.added, first.shortcut_hits, reads),
        (5, 0, 10),
        "the first run reads each file to hash it and again to parse it"
    );
    let (second, reads) = run(&mut opened.store);
    assert_eq!(
        (second.unchanged, second.shortcut_hits, reads, second.generation),
        (5, 5, 0, None),
        "nothing is read, nothing is written"
    );

    // an edit that changes the size is caught
    std::fs::write(ws.path("ws/src/m002.rs"), "pub fn changed_size_for_sure() {}\n").unwrap();
    let (third, reads) = run(&mut opened.store);
    assert_eq!((third.modified, third.shortcut_hits, reads), (1, 4, 2));

    // an edit that keeps size and mtime defeats the shortcut: the documented weakness
    let path = ws.path("ws/src/m003.rs");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let text = std::fs::read_to_string(&path).unwrap().replace("f3", "z3");
    std::fs::write(&path, text).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(before)
        .unwrap();
    let (fourth, _) = run(&mut opened.store);
    assert_eq!(fourth.modified, 0, "same size and same mtime: not seen");

    // racy: the clock says the files were read within two seconds of their mtime, so nothing is trusted
    let racy = SetClock(AtomicI64::new(pacenotes_index::SystemEpoch.now_ns()));
    let ws2 = workspace_of(&many(3));
    let mut opened2 = IndexStore::open(&ws2.path("index.db")).unwrap();
    let fs2 = RecordingFs::new(&ws2.path("ws"));
    let ix2 = Indexer {
        clock: &racy,
        ..Indexer::new(&fs2, &source, &deny)
    };
    ix2.run(&mut opened2.store, &CancelToken::new()).unwrap();
    let again = ix2.run(&mut opened2.store, &CancelToken::new()).unwrap();
    assert_eq!((again.unchanged, again.shortcut_hits), (3, 0), "a racy file is read again");
    // later, the clock has moved on: the unchanged files were restamped by the second run, so the third run trusts them
    racy.0.store(far_future, Ordering::SeqCst);
    let third = ix2.run(&mut opened2.store, &CancelToken::new()).unwrap();
    assert_eq!(third.shortcut_hits, 0, "the stamp holds the earlier read time, which was racy");
    let fourth = ix2.run(&mut opened2.store, &CancelToken::new()).unwrap();
    assert_eq!(
        fourth.shortcut_hits, 3,
        "read again after the clock moved on, restamped, and now trusted"
    );
}

#[test]
fn a_file_the_worker_crashes_on_is_quarantined_listed_at_tier_one_and_not_sent_again() {
    let ws = workspace(&[("ok.rs", "pub fn ok() {}\n"), ("bad.rs", "pub fn bad() {}\n")]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let deny = Denylist::new();
    let crashing = ParseSource {
        crash_on: Some("bad.rs"),
        ..ParseSource::default()
    };
    let ix = Indexer::new(&fs, &crashing, &deny);
    let first = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!(first.added, 2);
    assert_eq!(first.newly_quarantined.len(), 1);
    assert_eq!(first.newly_quarantined[0].0, p("bad.rs"));
    assert!(first.newly_quarantined[0].1.contains("hung"));
    assert_eq!(
        opened.store.counts().unwrap(),
        (2, 1),
        "bad.rs is listed with no symbols, ok.rs is fully indexed"
    );
    let held = opened.store.quarantined().unwrap();
    assert_eq!(held.len(), 1);
    assert!(held[&p("bad.rs")].1.contains("hung"));
    assert!(opened.store.dump_text().unwrap().contains("bad.rs"));

    // unchanged: not sent again
    let again = ParseSource {
        crash_on: Some("bad.rs"),
        ..ParseSource::default()
    };
    let second = Indexer::new(&fs, &again, &deny)
        .run(&mut opened.store, &CancelToken::new())
        .unwrap();
    assert!(again.extracted.lock().unwrap().is_empty() && second.newly_quarantined.is_empty());

    // changed: tried again, and when it works the quarantine is lifted
    std::fs::write(ws.path("ws/bad.rs"), "pub fn fixed() {}\n").unwrap();
    let healthy = ParseSource::default();
    let third = Indexer::new(&fs, &healthy, &deny)
        .run(&mut opened.store, &CancelToken::new())
        .unwrap();
    assert_eq!(third.modified, 1);
    assert!(opened.store.quarantined().unwrap().is_empty());
    assert_eq!(opened.store.counts().unwrap(), (2, 2));

    // a quarantined file that is deleted leaves no row behind
    let crash_again = ParseSource {
        crash_on: Some("bad.rs"),
        ..ParseSource::default()
    };
    std::fs::write(ws.path("ws/bad.rs"), "pub fn bad_third() {}\n").unwrap();
    Indexer::new(&fs, &crash_again, &deny)
        .run(&mut opened.store, &CancelToken::new())
        .unwrap();
    assert_eq!(opened.store.quarantined().unwrap().len(), 1);
    std::fs::remove_file(ws.path("ws/bad.rs")).unwrap();
    Indexer::new(&fs, &healthy, &deny)
        .run(&mut opened.store, &CancelToken::new())
        .unwrap();
    assert!(
        opened.store.quarantined().unwrap().is_empty(),
        "a deleted file's quarantine row goes with it"
    );
}

#[test]
fn cancel_in_any_phase_leaves_the_previous_generation_and_the_save_phase_is_the_swap() {
    let ws = workspace_of(&many(6));
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let (source, deny) = (ParseSource::default(), Denylist::new());
    Indexer::new(&fs, &source, &deny)
        .run(&mut opened.store, &CancelToken::new())
        .unwrap();
    let live = CancelToken::new();
    let before = opened.store.generation(&live).unwrap();

    for phase in ["walk", "read", "extract"] {
        std::fs::write(ws.path("ws/src/m001.rs"), format!("pub fn cancelled_in_{phase}() {{}}\n")).unwrap();
        let token = CancelToken::new();
        let rec = Recorder {
            on_phase: Some((
                match phase {
                    "walk" => "walk",
                    "read" => "read",
                    _ => "extract",
                },
                token.clone(),
            )),
            ..Recorder::default()
        };
        let ix = Indexer {
            progress: &rec,
            ..Indexer::new(&fs, &source, &deny)
        };
        assert_eq!(
            ix.run(&mut opened.store, &token),
            Err(IndexError::Cancelled),
            "cancelled at the start of {phase}"
        );
        assert_eq!(
            opened.store.generation(&live).unwrap(),
            before,
            "{phase}: the previous generation is untouched"
        );
        assert!(opened.store.search(&format!("cancelled_in_{phase}"), 3, &live).unwrap().is_empty());
    }

    // cancelled as the save phase begins: the commit is the swap and is not interrupted
    let token = CancelToken::new();
    let rec = Recorder {
        on_phase: Some(("save", token.clone())),
        ..Recorder::default()
    };
    let ix = Indexer {
        progress: &rec,
        ..Indexer::new(&fs, &source, &deny)
    };
    let report = ix.run(&mut opened.store, &token).unwrap();
    assert!(report.generation.is_some());
    assert!(opened.store.generation(&live).unwrap().0 > before.0);
}
