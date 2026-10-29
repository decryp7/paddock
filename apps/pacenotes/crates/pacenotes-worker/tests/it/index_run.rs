//! The whole path: the index over a real workspace with the real worker pool.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pacenotes_core::RepoPath;
use pacenotes_index::{Denylist, DirEntry, DiskFs, FsError, IndexStore, Indexer, WorkspaceFs};
use pacenotes_trap::{Observations, TRAP_QUERIES, generate};
use pacenotes_worker::{OsProbe, ParseWorkers};
use paddock_jobs::{CancelToken, Workers};
use paddock_testkit::Scratch;

use crate::support::{p, settings};

struct Recording {
    inner: DiskFs,
    reads: Mutex<Vec<PathBuf>>,
}

impl WorkspaceFs for Recording {
    fn list(&self, dir: Option<&RepoPath>) -> Result<Vec<DirEntry>, FsError> {
        self.inner.list(dir)
    }

    fn read(&self, path: &RepoPath, max: u64) -> Result<Vec<u8>, FsError> {
        let mut at = self.inner.root().to_path_buf();
        at.extend(path.as_str().split('/'));
        self.reads.lock().unwrap().push(at);
        self.inner.read(path, max)
    }
}

#[test]
fn a_crashing_file_among_many_is_quarantined_and_the_run_goes_on_with_a_pool_of_workers() {
    let scratch = Scratch::new();
    for i in 0..24 {
        scratch
            .write(&format!("ws/src/m{i:02}.rs"), format!("pub fn f{i}() {{}}\n"))
            .unwrap();
    }
    scratch.write("ws/src/poison.abort", "x").unwrap();
    scratch.write("ws/src/other.exit", "x").unwrap();
    let fs = DiskFs::new(&scratch.path("ws")).unwrap();
    let workers = ParseWorkers::new(settings(), Arc::new(OsProbe));
    let deny = Denylist::new();
    let ix = Indexer {
        parse_workers: Workers::new(4),
        read_workers: Workers::new(2),
        ..Indexer::new(&fs, &workers, &deny)
    };
    let mut opened = IndexStore::open(&scratch.path("index.db")).unwrap();
    let report = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!(report.added, 26);
    let mut quarantined: Vec<_> = report.newly_quarantined.iter().map(|(path, _)| path.to_string()).collect();
    quarantined.sort();
    assert_eq!(quarantined, ["src/other.exit", "src/poison.abort"]);
    assert_eq!(
        opened.store.counts().unwrap(),
        (26, 24),
        "every healthy file has its symbol; the two crashed ones are listed without"
    );
    assert!(workers.started() >= 2, "the pool used several processes");

    // the crashed files are not sent again; nothing changed, so nothing is read and nothing is parsed
    let again = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    assert_eq!((again.unchanged, again.newly_quarantined.len(), again.generation), (26, 0, None));
    assert_eq!(opened.store.quarantined().unwrap().len(), 2);
}

#[test]
fn indexing_the_trap_through_the_real_worker_runs_nothing_leaks_nothing_and_escapes_nothing() {
    let scratch = Scratch::new();
    let trap = generate(&scratch).unwrap();
    let fs = Recording {
        inner: DiskFs::new(&trap.workspace).unwrap(),
        reads: Mutex::new(Vec::new()),
    };
    let workers = ParseWorkers::new(settings(), Arc::new(OsProbe));
    let deny = Denylist::new();
    let ix = Indexer {
        parse_workers: Workers::new(2),
        ..Indexer::new(&fs, &workers, &deny)
    };
    let mut opened = IndexStore::open(&scratch.path("index.db")).unwrap();
    let report = ix.run(&mut opened.store, &CancelToken::new()).unwrap();
    assert!(report.added >= 6 && report.denied >= 3, "{report:?}");

    let known = opened.store.known_files().unwrap();
    let mut served = opened.store.dump_text().unwrap().into_bytes();
    for k in &known {
        for text in ix.chunk_texts(&opened.store, &k.path).unwrap() {
            served.extend_from_slice(text.as_bytes());
        }
    }
    let live = CancelToken::new();
    let search_replies = TRAP_QUERIES
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
        children_started: Vec::new(),
        files_read: fs.reads.lock().unwrap().clone(),
        indexed_files: known.iter().map(|k| k.path.to_string()).collect(),
        everything_stored_or_replied: served,
        search_replies,
    };
    assert_eq!(seen.verdict(&trap), vec![]);
    assert!(!seen.files_read.iter().any(|p| p.ends_with(".env")));
    assert!(!trap.marker.exists());
    // the worker processes the host started are the only children, and the index asked for no others: the macro and the build
    // script of the trap were parsed as text
    assert!(workers.started() >= 1);
    assert!(ix.chunk_texts(&opened.store, &p(".env")).is_err());
}
