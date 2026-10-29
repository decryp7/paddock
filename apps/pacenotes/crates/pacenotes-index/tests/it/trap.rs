//! Audit PN1, the trap repository (plan A3a): indexing it runs nothing, leaks nothing and escapes nothing. This test was the
//! ignored placeholder of `pacenotes-trap`; it lives here because the index cannot be a dependency of the fixture crate.

use pacenotes_index::{Denylist, IndexLimits, IndexStore, Indexer};
use pacenotes_trap::{Observations, TRAP_QUERIES, generate};
use paddock_jobs::CancelToken;
use paddock_testkit::Scratch;

use crate::support::{ParseSource, RecordingFs, p};

#[test]
fn indexing_the_trap_runs_nothing_leaks_nothing_and_escapes_nothing() {
    let scratch = Scratch::new();
    let trap = generate(&scratch).unwrap();
    let fs = RecordingFs::new(&trap.workspace);
    let source = ParseSource::default();
    let deny = Denylist::new();
    let ix = Indexer {
        fs: &fs,
        source: &source,
        denylist: &deny,
        limits: IndexLimits::default(),
    };
    let mut opened = IndexStore::open(&scratch.path("index.db")).unwrap();
    let report = ix.run(&mut opened.store, &CancelToken::new()).unwrap();

    // what the walk left out is counted: .env and everything under .git (and the nested gitfile), and every link
    assert!(report.denied >= 3, ".env, .git and sub/.git are withheld: {report:?}");
    #[cfg(unix)]
    assert_eq!(report.links, 5, "the folder, file, root and loop links are skipped, never followed");
    assert!(report.added >= 6, "the ordinary files of the workspace are indexed: {report:?}");

    // the files table, every table's text, the store's own search for each trap query, and the text of every chunk
    let known = opened.store.known_files().unwrap();
    let mut served = opened.store.dump_text().unwrap().into_bytes();
    let mut texts = Vec::new();
    for k in &known {
        for text in ix.chunk_texts(&opened.store, &k.path).unwrap() {
            served.extend_from_slice(text.as_bytes());
            texts.push(text);
        }
    }
    let live = CancelToken::new();
    let search_replies: Vec<String> = TRAP_QUERIES
        .iter()
        .map(|q| {
            opened
                .store
                .search(q, 50, &live)
                .unwrap()
                .iter()
                .map(|h| format!("{} {} {}", h.kind, h.text, h.path))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();

    let seen = Observations {
        children_started: Vec::new(), // the index has no way to start one: no process API, and the source double records none
        files_read: fs.reads(),
        indexed_files: known.iter().map(|k| k.path.to_string()).collect(),
        everything_stored_or_replied: served,
        search_replies,
    };
    let breaches = seen.verdict(&trap);
    assert!(breaches.is_empty(), "{breaches:?}");

    // the same facts, said directly
    assert!(
        !seen.files_read.iter().any(|p| p.ends_with(".env")),
        "the secret file was never opened"
    );
    assert!(
        !seen
            .indexed_files
            .iter()
            .any(|f| f == ".env" || f.starts_with(".git/") || f.starts_with("sub/")),
        "{:?}",
        seen.indexed_files
    );
    assert!(!trap.marker.exists(), "no build script, hook, wrapper, macro or driver ran");
    assert!(source.extracted.lock().unwrap().iter().all(|f| f != ".env"));
    assert!(
        ix.chunk_texts(&opened.store, &p(".env")).is_err(),
        "the read API refuses the secret file"
    );
    let workspace = trap.workspace.canonicalize().unwrap();
    assert!(
        seen.files_read
            .iter()
            .all(|f| f.canonicalize().is_ok_and(|real| real.starts_with(&workspace))),
        "every read stayed inside the workspace"
    );
    assert!(texts.iter().any(|t| t.contains("pub fn visible")), "the ordinary source is served");
}

#[test]
fn a_second_run_over_the_trap_is_stable() {
    let scratch = Scratch::new();
    let trap = generate(&scratch).unwrap();
    let fs = RecordingFs::new(&trap.workspace);
    let (source, deny) = (ParseSource::default(), Denylist::new());
    let ix = Indexer {
        fs: &fs,
        source: &source,
        denylist: &deny,
        limits: IndexLimits::default(),
    };
    let mut opened = IndexStore::open(&scratch.path("index.db")).unwrap();
    let first = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    let second = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!((second.unchanged, second.generation), (first.added, None));
    assert!(!trap.marker.exists());
}
