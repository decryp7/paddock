//! The store: the rebuild rule ("a damaged database is rebuilt, not trusted"), persistence, and the core `Store` contract.

use pacenotes_core::contract::store_contract;
use pacenotes_core::ports::Store;
use pacenotes_index::{Denylist, IndexLimits, IndexStore, Indexer, RebuildReason};
use paddock_jobs::CancelToken;

use crate::support::{ParseSource, RecordingFs, p, workspace};

const LIB: &str = "use std::fmt;\n\npub fn one() {}\n\npub fn two() {}\n";

fn indexed(ws: &paddock_testkit::Scratch, store: &mut IndexStore) {
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
    .unwrap();
}

#[test]
fn a_new_store_is_empty_and_a_reopened_one_keeps_its_data() {
    let ws = workspace(&[("lib.rs", LIB)]);
    let db = ws.path("index.db");
    {
        let mut opened = IndexStore::open(&db).unwrap();
        assert!(opened.rebuilt.is_none());
        assert_eq!(opened.store.counts().unwrap(), (0, 0));
        indexed(&ws, &mut opened.store);
        assert_eq!(opened.store.counts().unwrap(), (1, 2));
        assert_eq!(opened.store.path(), db);
    }
    let again = IndexStore::open(&db).unwrap();
    assert!(again.rebuilt.is_none(), "a good file is trusted");
    assert_eq!(again.store.counts().unwrap(), (1, 2));
    assert_eq!(again.store.next_file_sequence().unwrap(), 1);
    assert_eq!(again.store.language_of(&p("lib.rs")).unwrap().unwrap().id(), "rust");
}

#[test]
fn a_file_that_is_not_a_database_is_moved_aside_and_rebuilt() {
    let ws = workspace(&[]);
    let db = ws.path("index.db");
    std::fs::write(&db, "this is not a database, it is a note about one").unwrap();
    let opened = IndexStore::open(&db).unwrap();
    assert_eq!(opened.rebuilt, Some(RebuildReason::NotADatabase));
    assert_eq!(opened.store.counts().unwrap(), (0, 0));
    assert!(
        std::fs::read_to_string(ws.path("index.db.corrupt"))
            .unwrap()
            .contains("a note about one"),
        "the old file is kept aside"
    );
    assert!(opened.rebuilt.unwrap().to_string().contains("not a database"));
}

#[test]
fn a_truncated_or_overwritten_database_is_not_trusted() {
    let ws = workspace(&[("lib.rs", LIB)]);
    let db = ws.path("index.db");
    {
        let mut opened = IndexStore::open(&db).unwrap();
        indexed(&ws, &mut opened.store);
    }
    let bytes = std::fs::read(&db).unwrap();
    assert!(bytes.len() > 4096);
    for damage in ["truncated", "zeroed pages"] {
        let mut broken = bytes.clone();
        if damage == "truncated" {
            broken.truncate(bytes.len() / 2);
        } else {
            for b in &mut broken[4096..] {
                *b = 0;
            }
        }
        let case = ws.path(format!("case-{}.db", damage.len()).as_str());
        std::fs::write(&case, &broken).unwrap();
        let opened = IndexStore::open(&case).unwrap();
        assert!(opened.rebuilt.is_some(), "{damage}: the database was rebuilt");
        assert_eq!(
            opened.store.counts().unwrap(),
            (0, 0),
            "{damage}: nothing of the damaged file is trusted"
        );
    }
}

#[test]
fn another_schema_version_or_none_is_rebuilt() {
    let ws = workspace(&[]);
    for (name, version) in [
        ("future", Some("2")),
        ("past", Some("0")),
        ("text", Some("one")),
        ("missing-row", None),
    ] {
        let db = ws.path(&format!("{name}.db"));
        {
            let opened = IndexStore::open(&db).unwrap();
            drop(opened);
            let conn = rusqlite::Connection::open(&db).unwrap();
            match version {
                Some(v) => conn
                    .execute("UPDATE meta SET value = ?1 WHERE key = 'schema_version'", [v])
                    .unwrap(),
                None => conn.execute("DELETE FROM meta WHERE key = 'schema_version'", []).unwrap(),
            };
        }
        let opened = IndexStore::open(&db).unwrap();
        assert!(
            matches!(opened.rebuilt, Some(RebuildReason::SchemaVersion(_))),
            "{name}: {:?}",
            opened.rebuilt
        );
    }
    // a database without the meta table at all
    let db = ws.path("foreign.db");
    rusqlite::Connection::open(&db).unwrap().execute("CREATE TABLE t(x)", []).unwrap();
    assert_eq!(IndexStore::open(&db).unwrap().rebuilt, Some(RebuildReason::SchemaVersion(None)));
    // an empty file is a new store
    let empty = ws.path("empty.db");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(IndexStore::open(&empty).unwrap().rebuilt, Some(RebuildReason::SchemaVersion(None)));
    assert!(RebuildReason::SchemaVersion(Some(2)).to_string().contains("version 2"));
    assert!(RebuildReason::SchemaVersion(None).to_string().contains("no schema version"));
    assert!(RebuildReason::Damaged("page 3".into()).to_string().contains("damaged (page 3)"));
}

#[test]
fn a_second_rebuild_replaces_the_file_kept_aside() {
    let ws = workspace(&[]);
    let db = ws.path("index.db");
    std::fs::write(&db, "first garbage").unwrap();
    drop(IndexStore::open(&db).unwrap());
    std::fs::write(&db, "second garbage").unwrap();
    drop(IndexStore::open(&db).unwrap());
    assert_eq!(std::fs::read_to_string(ws.path("index.db.corrupt")).unwrap(), "second garbage");
}

#[test]
fn the_store_honours_the_core_store_contract() {
    let ws = workspace(&[("src/lib.rs", LIB)]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    indexed(&ws, &mut opened.store);
    store_contract(&opened.store, &p("src/lib.rs"), &p("src/missing.rs"));
    let live = CancelToken::new();
    assert_eq!(opened.store.generation(&live).unwrap().0, 1);
}

#[test]
fn search_finds_files_and_symbols_case_insensitively_and_treats_wildcards_as_text() {
    let ws = workspace(&[
        ("src/shapes.rs", "pub struct Circle;\npub fn area_of() {}\n"),
        ("notes_100%.txt", "plain"),
    ]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    indexed(&ws, &mut opened.store);
    let live = CancelToken::new();
    let hits = |q: &str| opened.store.search(q, 10, &live).unwrap();
    assert!(hits("SHAPES").iter().any(|h| h.kind == "file" && h.text == "src/shapes.rs"));
    assert!(
        hits("circle")
            .iter()
            .any(|h| h.kind == "symbol" && h.text == "Circle" && h.path == p("src/shapes.rs"))
    );
    assert_eq!(hits("100%").len(), 1, "a percent sign is a character, not a wildcard");
    assert!(
        hits("_").iter().all(|h| h.text.contains('_')),
        "an underscore is a character, not a wildcard"
    );
    assert!(hits("zzz").is_empty());
    assert_eq!(opened.store.search("s", 1, &live).unwrap().len(), 1, "the limit applies");
    let dead = CancelToken::new();
    dead.cancel();
    assert!(opened.store.search("a", 1, &dead).is_err());
    assert!(opened.store.dump_text().unwrap().contains("src/shapes.rs"));
}

#[test]
fn stored_import_targets_pass_through_redaction() {
    use pacenotes_core::{FileIdAllocator, Fingerprint, Language, Span};
    use pacenotes_index::{Batch, FileFacts, ImportFact, Upsert};
    let ws = workspace(&[]);
    let mut opened = IndexStore::open(&ws.path("index.db")).unwrap();
    let span = Span::lines(1, 1).unwrap();
    let facts = FileFacts {
        language: Language::parse("go").unwrap(),
        symbols: vec![],
        imports: vec![
            ImportFact {
                target: "postgres://app:hunter2pw@db.example.invalid/app".into(),
                span,
            },
            ImportFact {
                target: "fmt".into(),
                span,
            },
        ],
        calls: vec![],
        chunks: vec![],
        partial: false,
        has_errors: false,
    };
    let batch = Batch {
        upserts: vec![Upsert {
            file_id: FileIdAllocator::new().issue().unwrap(),
            path: p("main.go"),
            fingerprint: Fingerprint::of_content(b"x"),
            size: 1,
            facts: Some(facts),
        }],
        next_file_sequence: 1,
        ..Batch::default()
    };
    opened.store.commit(&batch).unwrap();
    let dump = opened.store.dump_text().unwrap();
    assert!(dump.contains("postgres://[redacted]@db.example.invalid/app") && dump.contains("fmt"));
    assert!(!dump.contains("hunter2pw"));
}
