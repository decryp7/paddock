//! What a run does: unchanged, modified, deleted, limits, failures, cancel, and what the read API withholds.

use pacenotes_core::ports::Store;
use pacenotes_index::{Denylist, IndexError, IndexLimits, IndexStore, Indexer};
use paddock_jobs::CancelToken;

use crate::support::{ParseSource, RecordingFs, p, workspace};

fn indexer<'a>(fs: &'a RecordingFs, source: &'a ParseSource, deny: &'a Denylist) -> Indexer<'a> {
    Indexer {
        fs,
        source,
        denylist: deny,
        limits: IndexLimits::default(),
    }
}

#[test]
fn a_second_run_with_nothing_changed_writes_nothing_and_extracts_nothing() {
    let ws = workspace(&[
        ("a.rs", "pub fn a() {}\n"),
        ("b.go", "package b\nfunc B() {}\n"),
        ("data.json", "{}"),
    ]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let deny = Denylist::new();
    let source = ParseSource::default();
    let first = indexer(&fs, &source, &deny).run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!((first.added, first.tier1_only, first.generation.map(|g| g.0)), (3, 1, Some(1)));
    assert_eq!(source.extracted.lock().unwrap().len(), 3);
    let source2 = ParseSource::default();
    let second = indexer(&fs, &source2, &deny).run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!(
        (second.unchanged, second.generation),
        (3, None),
        "no new generation when nothing changed"
    );
    assert!(source2.extracted.lock().unwrap().is_empty());
    assert_eq!(
        opened.store.language_of(&p("data.json")).unwrap(),
        None,
        "a file in no known language is listed at tier 1"
    );
}

#[test]
fn modified_and_deleted_files_replace_and_remove_their_rows() {
    let ws = workspace(&[("a.rs", "pub fn old() {}\n"), ("gone.rs", "pub fn gone() {}\n")]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let deny = Denylist::new();
    let source = ParseSource::default();
    indexer(&fs, &source, &deny).run(&mut opened.store, &CancelToken::new()).unwrap();
    std::fs::write(ws.path("ws/a.rs"), "pub fn new_name() {}\n").unwrap();
    std::fs::remove_file(ws.path("ws/gone.rs")).unwrap();
    let r = indexer(&fs, &source, &deny).run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!((r.modified, r.deleted), (1, 1));
    let live = CancelToken::new();
    let hits = opened.store.search("old", 10, &live).unwrap();
    assert!(hits.is_empty(), "{hits:?}");
    assert!(!opened.store.search("new_name", 10, &live).unwrap().is_empty());
    assert!(opened.store.search("gone", 10, &live).unwrap().is_empty());
    assert_eq!(opened.store.counts().unwrap(), (1, 1));
}

#[test]
fn a_file_the_extractor_fails_on_is_recorded_and_the_rest_is_indexed() {
    let ws = workspace(&[("ok.rs", "pub fn ok() {}\n"), ("bad.rs", "pub fn bad() {}\n")]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let deny = Denylist::new();
    let source = ParseSource {
        fail_on: Some("bad.rs"),
        ..ParseSource::default()
    };
    let r = indexer(&fs, &source, &deny).run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!((r.added, r.extract_failure_count, r.tier1_only), (2, 1, 1));
    assert_eq!(r.extract_failures[0].0, p("bad.rs"));
    assert!(r.extract_failures[0].1.contains("crashed"));
    assert_eq!(opened.store.counts().unwrap(), (2, 1), "the failing file is listed with no symbols");
    // only the first failures are named; the rest are counted
    let ws2 = workspace(&[("x1.rs", "fn a() {}\n"), ("x2.rs", "fn b() {}\n")]);
    let mut opened2 = IndexStore::open(&ws2.path("index.db")).unwrap();
    let fs2 = RecordingFs::new(&ws2.path("ws"));
    let failing = ParseSource {
        fail_on: Some("x1.rs"),
        ..ParseSource::default()
    };
    let limited = Indexer {
        limits: IndexLimits {
            max_failures_named: 0,
            ..IndexLimits::default()
        },
        ..indexer(&fs2, &failing, &deny)
    };
    let r2 = limited.run(&mut opened2.store, &CancelToken::new()).unwrap();
    assert_eq!((r2.extract_failure_count, r2.extract_failures.len()), (1, 0));
}

#[test]
fn big_files_and_unreadable_ones_are_counted_not_indexed() {
    let ws = workspace(&[("small.rs", "pub fn s() {}\n"), ("big.rs", &"// filler\n".repeat(100))]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let deny = Denylist::new();
    let source = ParseSource::default();
    let small_limit = Indexer {
        limits: IndexLimits {
            max_file_bytes: 100,
            ..IndexLimits::default()
        },
        ..indexer(&fs, &source, &deny)
    };
    let r = small_limit.run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!((r.added, r.too_large), (1, 1));
    assert!(opened.store.search("big", 5, &CancelToken::new()).unwrap().is_empty());
}

#[test]
fn a_cancel_before_the_commit_leaves_the_previous_generation_untouched() {
    let ws = workspace(&[("a.rs", "pub fn a() {}\n")]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let deny = Denylist::new();
    indexer(&fs, &ParseSource::default(), &deny)
        .run(&mut opened.store, &CancelToken::new())
        .unwrap();
    let live = CancelToken::new();
    let before = opened.store.generation(&live).unwrap();

    std::fs::write(ws.path("ws/b.rs"), "pub fn b() {}\n").unwrap();
    std::fs::write(ws.path("ws/a.rs"), "pub fn a2() {}\n").unwrap();
    let dead = CancelToken::new();
    dead.cancel();
    assert_eq!(
        indexer(&fs, &ParseSource::default(), &deny).run(&mut opened.store, &dead),
        Err(IndexError::Cancelled)
    );
    // cancelled in the middle of extraction
    let token = CancelToken::new();
    let source = ParseSource {
        cancel_on: Some(("b.rs", token.clone())),
        ..ParseSource::default()
    };
    assert_eq!(
        indexer(&fs, &source, &deny).run(&mut opened.store, &token),
        Err(IndexError::Cancelled)
    );
    assert_eq!(opened.store.generation(&live).unwrap(), before);
    assert_eq!(opened.store.counts().unwrap(), (1, 1), "nothing of the cancelled run is visible");
    assert!(opened.store.search("a2", 5, &live).unwrap().is_empty());
    // and the next run, uncancelled, picks it all up
    let r = indexer(&fs, &ParseSource::default(), &deny)
        .run(&mut opened.store, &CancelToken::new())
        .unwrap();
    assert_eq!((r.added, r.modified), (1, 1));
}

#[test]
fn a_workspace_that_cannot_be_walked_is_an_error() {
    let ws = workspace(&[]);
    assert!(pacenotes_index::DiskFs::new(&ws.path("missing")).is_err());
}

#[test]
fn the_read_api_withholds_denied_files_and_masks_credentials_in_what_it_serves() {
    let ws = workspace(&[
        (
            "src/lib.rs",
            "pub fn connect() {\n    let password = \"hunter2hunter2\";\n    let url = \"db\";\n}\n",
        ),
        (".env", "API_TOKEN=PN1-DUMMY\n"),
    ]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let fs = RecordingFs::new(&ws.path("ws"));
    let deny = Denylist::new();
    let source = ParseSource::default();
    let ix = indexer(&fs, &source, &deny);
    let r = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!((r.added, r.denied), (1, 1), ".env is withheld and counted");
    assert!(!fs.reads().iter().any(|p| p.ends_with(".env")), "a denied file is never opened");
    let texts = ix.chunk_texts(&opened.store, &p("src/lib.rs")).unwrap();
    assert!(texts.iter().any(|t| t.contains("[redacted]")), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("hunter2")));
    assert_eq!(ix.chunk_texts(&opened.store, &p(".env")), Err(IndexError::Denied(p(".env"))));
    assert!(IndexError::Denied(p(".env")).to_string().contains("withheld"));
    assert!(
        ix.chunk_texts(&opened.store, &p("src/missing.rs"))
            .unwrap_err()
            .to_string()
            .starts_with("workspace:")
    );
}
