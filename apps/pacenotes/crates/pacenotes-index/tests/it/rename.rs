//! The regression tests of the plan (A3a): a renamed file keeps its symbol ids, and moving a file away then restoring the path
//! never gives two files one id.

use pacenotes_core::ports::Store;
use pacenotes_index::{Denylist, IndexLimits, IndexStore, Indexer};
use paddock_jobs::CancelToken;

use crate::support::{ParseSource, RecordingFs, p, workspace};

const LIB: &str = "pub struct Shape;\n\nimpl Shape {\n    pub fn area(&self) -> u32 {\n        helper(2)\n    }\n}\n\npub fn helper(x: u32) -> u32 {\n    x\n}\n";

fn run(ws: &paddock_testkit::Scratch, store: &mut IndexStore) -> pacenotes_index::IndexReport {
    let fs = RecordingFs::new(&ws.path("ws"));
    let source = ParseSource::default();
    let deny = Denylist::new();
    Indexer {
        fs: &fs,
        source: &source,
        denylist: &deny,
        limits: IndexLimits::default(),
    }
    .run(store, &CancelToken::new())
    .unwrap()
}

#[test]
fn renaming_a_file_keeps_its_file_id_and_every_symbol_id() {
    let ws = workspace(&[("src/old_name.rs", LIB), ("other.rs", "pub fn other() {}\n")]);
    let db = ws.path("index.db");
    let mut opened = IndexStore::open(&db).unwrap();
    let first = run(&ws, &mut opened.store);
    assert_eq!((first.added, first.generation.map(|g| g.0)), (2, Some(1)));
    let live = CancelToken::new();
    let id_before = opened.store.file_id(&p("src/old_name.rs"), &live).unwrap().unwrap();
    let symbols_before = opened.store.symbol_ids(&p("src/old_name.rs")).unwrap();
    assert!(symbols_before.len() >= 3, "{symbols_before:?}");

    std::fs::rename(ws.path("ws/src/old_name.rs"), ws.path("ws/src/new_name.rs")).unwrap();
    let second = run(&ws, &mut opened.store);
    assert_eq!((second.renamed, second.added, second.deleted, second.modified), (1, 0, 0, 0));
    assert_eq!(second.generation.map(|g| g.0), Some(2));

    assert_eq!(
        opened.store.file_id(&p("src/new_name.rs"), &live).unwrap(),
        Some(id_before),
        "the id moved with the file"
    );
    assert_eq!(opened.store.file_id(&p("src/old_name.rs"), &live).unwrap(), None);
    assert_eq!(
        opened.store.symbol_ids(&p("src/new_name.rs")).unwrap(),
        symbols_before,
        "every symbol id survived the rename"
    );
}

#[test]
fn moving_a_file_away_and_restoring_the_path_gives_two_different_ids() {
    let ws = workspace(&[("a.rs", LIB)]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    run(&ws, &mut opened.store);
    let live = CancelToken::new();
    let original = opened.store.file_id(&p("a.rs"), &live).unwrap().unwrap();

    // git mv a.rs b.rs, then a new file appears at a.rs with the same content
    std::fs::rename(ws.path("ws/a.rs"), ws.path("ws/b.rs")).unwrap();
    run(&ws, &mut opened.store);
    std::fs::write(ws.path("ws/a.rs"), LIB).unwrap();
    let report = run(&ws, &mut opened.store);
    assert_eq!((report.added, report.renamed), (1, 0));

    let at_b = opened.store.file_id(&p("b.rs"), &live).unwrap().unwrap();
    let at_a = opened.store.file_id(&p("a.rs"), &live).unwrap().unwrap();
    assert_eq!(at_b, original, "the moved file kept its id");
    assert_ne!(at_a, at_b, "the new file at the old path has its own id");
    assert_ne!(
        opened.store.symbol_ids(&p("a.rs")).unwrap(),
        opened.store.symbol_ids(&p("b.rs")).unwrap()
    );
}

#[test]
fn an_edited_file_keeps_its_id_and_symbols_that_did_not_change_keep_theirs() {
    let ws = workspace(&[("lib.rs", LIB)]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    run(&ws, &mut opened.store);
    let before = opened.store.symbol_ids(&p("lib.rs")).unwrap();
    std::fs::write(ws.path("ws/lib.rs"), format!("{LIB}\npub fn added() {{}}\n")).unwrap();
    let report = run(&ws, &mut opened.store);
    assert_eq!((report.modified, report.added), (1, 0));
    let after = opened.store.symbol_ids(&p("lib.rs")).unwrap();
    for kept in &before {
        assert!(after.contains(kept), "{kept:?} survived the edit");
    }
    assert_eq!(after.len(), before.len() + 1);
}
